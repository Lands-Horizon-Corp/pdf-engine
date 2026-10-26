//! Bounded, copy-free template rendering.
//!
//! The request JSON is shared with minijinja through a lazy view (`JsonNode`) instead of being
//! deep-copied into a second value tree. Output is written through a capped writer and
//! execution is limited by fuel, so a hostile or accidental template (nested loops over a large
//! array) fails with a 400 instead of exhausting memory or a blocking thread.

use crate::error::AppError;
use minijinja::{
    Environment, ErrorKind, Value,
    value::{Enumerator, Object, ObjectRepr},
};
use std::{
    io::{self, Write},
    sync::{Arc, LazyLock},
};

/// Output cap in bytes (`TEMPLATE_MAX_OUTPUT_BYTES`, default 64 MiB).
static MAX_OUTPUT: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("TEMPLATE_MAX_OUTPUT_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        // 5 GiB; capped at the platform's address space (4 GiB on 32-bit targets like wasm32).
        .unwrap_or_else(|| usize::try_from(5u64 << 30).unwrap_or(usize::MAX))
});

/// Template instruction budget (`TEMPLATE_FUEL`, default 50 million).
static FUEL: LazyLock<u64> = LazyLock::new(|| {
    std::env::var("TEMPLATE_FUEL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50_000_000)
});

fn new_env() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::Html);
    env.set_recursion_limit(64);
    // `{{ text|markdown }}` and `{% filter markdown %}...{% endfilter %}`: Markdown to HTML (see markdown.rs).
    env.add_filter("markdown", |text: &str| {
        Value::from_safe_string(crate::markdown::to_html(text))
    });
    // `{{ "x^2"|math }}` (inline) and `{{ formula|math(display=true) }}`: LaTeX math as laid-out HTML (see math.rs).
    env.add_filter(
        "math",
        |text: &str, kwargs: minijinja::value::Kwargs| -> Result<Value, minijinja::Error> {
            let display = kwargs.get::<Option<bool>>("display")?.unwrap_or(false);
            kwargs.assert_all_used()?;
            Ok(Value::from_safe_string(crate::math::to_html(text, display)))
        },
    );
    env.set_fuel(Some(*FUEL));
    env
}

static ENV: LazyLock<Environment<'static>> = LazyLock::new(new_env);

/// A node inside a shared `serde_json::Value`, resolved lazily as the template walks it.
///
/// `ptr` always points inside `_root`'s heap allocation. The tree is never mutated and `_root`
/// keeps it alive for as long as any node exists, so the pointer cannot dangle.
#[derive(Debug)]
struct JsonNode {
    _root: Arc<serde_json::Value>,
    ptr: *const serde_json::Value,
}

// SAFETY: the pointee is immutable `serde_json::Value` (Send + Sync) owned by `_root`.
unsafe impl Send for JsonNode {}
unsafe impl Sync for JsonNode {}

impl JsonNode {
    fn get(&self) -> &serde_json::Value {
        // SAFETY: see the type docs.
        unsafe { &*self.ptr }
    }

    /// Scalars become plain values; arrays and objects become lazy views.
    fn wrap(root: &Arc<serde_json::Value>, v: &serde_json::Value) -> Value {
        use serde_json::Value as J;
        match v {
            J::Null => Value::from(()),
            J::Bool(b) => Value::from(*b),
            J::Number(n) => n
                .as_i64()
                .map(Value::from)
                .or_else(|| n.as_u64().map(Value::from))
                .unwrap_or_else(|| Value::from(n.as_f64().unwrap_or(f64::NAN))),
            J::String(s) => Value::from(s.as_str()),
            J::Array(_) | J::Object(_) => Value::from_object(JsonNode {
                _root: Arc::clone(root),
                ptr: v,
            }),
        }
    }
}

impl Object for JsonNode {
    fn repr(self: &Arc<Self>) -> ObjectRepr {
        match self.get() {
            serde_json::Value::Array(_) => ObjectRepr::Seq,
            _ => ObjectRepr::Map,
        }
    }

    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        let child = match self.get() {
            serde_json::Value::Array(a) => a.get(key.as_usize()?)?,
            serde_json::Value::Object(m) => m.get(key.as_str()?)?,
            _ => return None,
        };
        Some(JsonNode::wrap(&self._root, child))
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        match self.get() {
            serde_json::Value::Array(a) => Enumerator::Seq(a.len()),
            serde_json::Value::Object(m) => {
                Enumerator::Values(m.keys().map(|k| Value::from(k.as_str())).collect())
            }
            _ => Enumerator::NonEnumerable,
        }
    }
}

/// `Write` that refuses bytes past `limit`.
struct CappedWriter {
    buf: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl Write for CappedWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.buf.len() + data.len() > self.limit {
            self.exceeded = true;
            return Err(io::Error::other("template output limit exceeded"));
        }
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Render `template_str` against `data` on the current (blocking) thread.
pub fn render(template_str: &str, data: serde_json::Value) -> Result<String, AppError> {
    render_with(template_str, data, &[])
}

/// Most partials one render accepts, and their total size.
const MAX_PARTIALS: usize = 100;
const MAX_PARTIALS_BYTES: usize = 5 * 1024 * 1024;

/// Like [`render`], with named partial templates the template can `{% include "name" %}`,
/// `{% extends "name" %}` or `{% import "name" as x %}`.
pub fn render_with(
    template_str: &str,
    data: serde_json::Value,
    partials: &[(String, String)],
) -> Result<String, AppError> {
    render_inner(template_str, data, partials, *MAX_OUTPUT)
}

#[cfg(test)]
fn render_with_limit(
    template_str: &str,
    data: serde_json::Value,
    limit: usize,
) -> Result<String, AppError> {
    render_inner(template_str, data, &[], limit)
}

/// `{% pagebreak %}` starts a new page: it becomes an empty block that breaks the page after itself.
fn expand_pagebreaks(src: &str) -> std::borrow::Cow<'_, str> {
    const BREAK: &str = r#"<div style="page-break-after:always;break-after:page;height:0;margin:0;padding:0;border:0"></div>"#;
    if !src.contains("pagebreak") {
        return std::borrow::Cow::Borrowed(src);
    }
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(open) = rest.find("{%") {
        let Some(close) = rest[open..].find("%}").map(|i| open + i) else {
            break;
        };
        let inner =
            rest[open + 2..close].trim_matches(|c: char| c == '-' || c == '+' || c.is_whitespace());
        out.push_str(&rest[..open]);
        if inner == "pagebreak" {
            out.push_str(BREAK);
        } else {
            out.push_str(&rest[open..close + 2]);
        }
        rest = &rest[close + 2..];
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
}

fn check_partials(partials: &[(String, String)]) -> Result<(), AppError> {
    if partials.len() > MAX_PARTIALS {
        return Err(AppError::InvalidField(format!(
            "partials: at most {MAX_PARTIALS} are allowed"
        )));
    }
    let mut total = 0usize;
    for (name, src) in partials {
        let valid = !name.is_empty()
            && name.len() <= 64
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_./-".contains(c))
            && !name.starts_with(['.', '/']);
        if !valid {
            return Err(AppError::InvalidField(format!(
                "partials: `{name}` is not a valid name (letters, digits, `_`, `-`, `.` and `/`, up to 64)"
            )));
        }
        total = total.saturating_add(src.len());
    }
    if total > MAX_PARTIALS_BYTES {
        return Err(AppError::PayloadTooLarge(MAX_PARTIALS_BYTES));
    }
    Ok(())
}

fn render_inner(
    template_str: &str,
    data: serde_json::Value,
    partials: &[(String, String)],
    limit: usize,
) -> Result<String, AppError> {
    let root = Arc::new(data);
    let ctx = JsonNode::wrap(&root, &root);
    let source = expand_pagebreaks(template_str);
    let html = if partials.is_empty() {
        let tmpl = ENV.template_from_str(&source)?;
        run(&tmpl, ctx, template_str.len(), limit)?
    } else {
        check_partials(partials)?;
        let mut env = new_env();
        for (name, src) in partials {
            env.add_template_owned(name.clone(), expand_pagebreaks(src).into_owned())?;
        }
        let tmpl = env.template_from_str(&source)?;
        run(&tmpl, ctx, template_str.len(), limit)?
    };
    // `<barcode>` tags become vector shapes, and `<math>` tags laid-out formulas, after the values are in.
    let html = match crate::barcode::expand(&html)? {
        std::borrow::Cow::Borrowed(_) => html,
        std::borrow::Cow::Owned(expanded) => expanded,
    };
    Ok(crate::math::expand(&html).unwrap_or(html))
}

fn run(
    tmpl: &minijinja::Template<'_, '_>,
    ctx: Value,
    template_len: usize,
    limit: usize,
) -> Result<String, AppError> {
    let mut out = CappedWriter {
        // Output is usually a small multiple of the template; avoid early regrowth.
        buf: Vec::with_capacity(template_len.saturating_mul(2).min(limit)),
        limit,
        exceeded: false,
    };
    match tmpl.render_captured_to(ctx, &mut out) {
        Ok(_) => String::from_utf8(out.buf)
            .map_err(|_| AppError::InvalidField("template produced invalid UTF-8".into())),
        Err(_) if out.exceeded => Err(AppError::TemplateOutputTooLarge(out.limit)),
        Err(e) if e.kind() == ErrorKind::OutOfFuel => Err(AppError::TemplateTooExpensive),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_markdown_filter_and_block() {
        let html = render_with("{{ t|markdown }}", json!({ "t": "**a** <b>b</b>" }), &[]).unwrap();
        assert_eq!(
            html.trim(),
            "<p><strong>a</strong> &lt;b&gt;b&lt;/b&gt;</p>"
        );
        // Written in the template, with a value inside.
        let html = render_with(
            "{% filter markdown %}# Hi {{ n }}\n\n- x{% endfilter %}",
            json!({ "n": "Ana" }),
            &[],
        )
        .unwrap();
        assert!(
            html.contains("<h1>Hi Ana</h1>") && html.contains("<li>x</li>"),
            "{html}"
        );
        // The text is escaped like any value: markup in a value never reaches the output as markup.
        let html = render_with(
            "{% filter markdown %}{{ n }}{% endfilter %}",
            json!({ "n": "<img src=x>" }),
            &[],
        )
        .unwrap();
        assert!(!html.contains("<img"), "{html}");
        // Without the filter a value is only escaped, never read as Markdown.
        assert_eq!(
            render_with("{{ t }}", json!({ "t": "**a**" }), &[]).unwrap(),
            "**a**"
        );
    }

    #[test]
    fn autoescapes_and_walks_nested_data() {
        let out = render(
            "{% for r in rows %}<p>{{ r.name }}:{{ r.n }}</p>{% endfor %}{{ rows|length }}",
            json!({ "rows": [{ "name": "<b>", "n": 1 }, { "name": "你", "n": 2.5 }] }),
        )
        .unwrap();
        assert_eq!(out, "<p>&lt;b&gt;:1</p><p>你:2.5</p>2");
    }

    #[test]
    fn iterates_object_keys() {
        let out = render(
            "{% for k in o %}{{ k }}={{ o[k] }};{% endfor %}",
            json!({ "o": { "a": 1, "b": 2 } }),
        )
        .unwrap();
        assert_eq!(out, "a=1;b=2;");
    }

    #[test]
    fn output_is_capped() {
        let tmpl = "{% for i in range(1000) %}{{ s }}{% endfor %}";
        let err = render_with_limit(tmpl, json!({ "s": "x".repeat(1024) }), 64 * 1024).unwrap_err();
        assert!(matches!(err, AppError::TemplateOutputTooLarge(_)));
    }

    fn ok(t: &str, d: serde_json::Value) -> String {
        render(t, d).unwrap()
    }

    #[test]
    fn scalars_keep_their_types() {
        let out = ok(
            "{{ n }}|{{ big }}|{{ f }}|{{ t }}|{{ z }}|{{ s }}",
            json!({ "n": -7, "big": u64::MAX, "f": 1.5, "t": true, "z": null, "s": "你好 🚀" }),
        );
        assert_eq!(out, "-7|18446744073709551615|1.5|True|None|你好 🚀");
    }

    #[test]
    fn missing_values_are_empty_and_conditionals_work() {
        let t = "[{{ nope }}][{{ a.b.c|default('-') }}]{% if on %}yes{% else %}no{% endif %}";
        assert_eq!(ok(t, json!({ "a": { "b": {} }, "on": false })), "[][-]no");
        assert_eq!(
            ok(t, json!({ "a": { "b": { "c": 3 } }, "on": true })),
            "[][3]yes"
        );
    }

    #[test]
    fn indexing_slicing_filters_and_loop_vars() {
        let d = json!({ "items": [{ "p": 2.5, "q": 2 }, { "p": 10.0, "q": 1 }], "nested": [[1, 2], [3]] });
        assert_eq!(
            ok(
                "{{ items[0].p }} {{ items[-1].q }} {{ nested[0][1] }}",
                d.clone()
            ),
            "2.5 1 2"
        );
        assert_eq!(
            ok(
                "{% for i in items %}{{ loop.index }}/{{ loop.length }},{% endfor %}",
                d.clone()
            ),
            "1/2,2/2,"
        );
        assert_eq!(ok("{{ items|map(attribute='q')|sum }}", d.clone()), "3");
        assert_eq!(
            ok("{% for r in nested %}{{ r|join('+') }};{% endfor %}", d),
            "1+2;3;"
        );
    }

    #[test]
    fn data_may_be_null_or_a_non_object() {
        assert_eq!(ok("static", serde_json::Value::Null), "static");
        assert_eq!(ok("{{ x }}", json!([1, 2])), "");
    }

    #[test]
    fn syntax_and_runtime_errors_are_template_errors() {
        assert!(matches!(
            render("{% if %}", json!({})),
            Err(AppError::Template(_))
        ));
        assert!(matches!(
            render("{{ 1 / 0 }}", json!({})),
            Err(AppError::Template(_))
        ));
    }

    #[test]
    fn runaway_templates_run_out_of_fuel() {
        let t = "{% for a in range(100000) %}{% for b in range(100000) %}{% endfor %}{% endfor %}";
        let err = render(t, json!({})).unwrap_err();
        assert!(err.to_string().contains("too expensive"), "{err}");
    }

    #[test]
    fn html_in_data_is_escaped_everywhere() {
        let out = ok(
            "{% for r in rows %}{{ r.a }}{{ r['b'] }}{% endfor %}",
            json!({ "rows": [{ "a": "<script>", "b": "\"&'" }] }),
        );
        assert!(
            !out.contains('<') && !out.contains('"') && !out.contains('\''),
            "{out}"
        );
    }

    #[test]
    fn large_inputs_render_correctly() {
        let rows: Vec<_> = (0..200_000).map(|i| json!({ "id": i, "v": "x" })).collect();
        let out = ok(
            "{% for r in rows %}{{ r.id }}{% endfor %}|{{ rows|length }}",
            json!({ "rows": rows }),
        );
        assert!(out.starts_with("0123456789"));
        assert!(out.ends_with("199999|200000"));
    }
}
