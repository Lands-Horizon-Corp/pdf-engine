//! LaTeX math to HTML, for `<math>x^2</math>` and the `math` filter.
//!
//! The renderer lays out HTML and CSS, not MathML, so a formula becomes nested inline boxes: fractions are stacked
//! boxes, scripts are smaller boxes lifted with a transform (a transform moves what is drawn, not the layout), roots
//! and bars are borders, matrices are grids. It reads the usual part of LaTeX math: letters, numbers, operators and
//! relations, Greek, `\frac`, `\sqrt`, `^` and `_`, `\sum \prod \int \lim` with limits, `\left ... \right`, matrices
//! and `cases`, accents, `\text` and the font commands (`\mathbf`, `\mathbb`, `\mathcal`, ...). It is not TeX: spacing
//! and the size of stretchy delimiters are approximations, and a command it does not know is shown as written.
//! Pure string work, so the browser build uses the same file.

/// Longest formula read, in characters; more is shown as plain text.
pub const MAX_FORMULA_CHARS: usize = 20_000;
const MAX_DEPTH: usize = 24;
const MAX_OUTPUT: usize = 1 << 20;

/// A formula as HTML. `display` centres it on its own line and sets big operators with their limits above and below.
pub fn to_html(latex: &str, display: bool) -> String {
    if latex.chars().count() > MAX_FORMULA_CHARS {
        return escape(latex);
    }
    let mut p = Parser {
        s: latex.chars().collect(),
        i: 0,
        depth: 0,
    };
    let row = p.row(&[]);
    let mut out = String::new();
    emit(
        &row,
        &Ctx {
            display,
            ..Ctx::default()
        },
        &mut out,
    );
    if out.len() > MAX_OUTPUT {
        return escape(latex);
    }
    if display {
        format!(
            "<div style=\"text-align:center;margin:0.7em 0;padding:0.3em 0;font-size:1.15em;font-family:'Noto Sans','Noto Sans Math',sans-serif\">{out}</div>"
        )
    } else {
        format!(
            "<span style=\"white-space:nowrap;font-family:'Noto Sans','Noto Sans Math',sans-serif\">{out}</span>"
        )
    }
}

/// Replace every `<math>...</math>` and `<math display>...</math>` in `html` with the formula as HTML. `None` when
/// there is none. The text inside is read as LaTeX after its HTML entities (`&lt;`, `&amp;`, ...) are decoded.
pub fn expand(html: &str) -> Option<String> {
    // Most documents have no formula: look for `<math` without copying the document.
    if !html
        .as_bytes()
        .windows(5)
        .any(|w| w.eq_ignore_ascii_case(b"<math"))
    {
        return None;
    }
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() + 256);
    let mut at = 0;
    let mut changed = false;
    while let Some(found) = lower[at..].find("<math") {
        let start = at + found;
        let after = start + "<math".len();
        // `<math>` or `<math display ...>`, not `<mathematics>`.
        let Some(tag_end) = html[after..].find('>').map(|i| after + i) else {
            break;
        };
        let attrs = &lower[after..tag_end];
        if !(attrs.is_empty() || attrs.starts_with(|c: char| c.is_ascii_whitespace() || c == '/')) {
            out.push_str(&html[at..after]);
            at = after;
            continue;
        }
        let Some(close) = lower[tag_end + 1..]
            .find("</math>")
            .map(|i| tag_end + 1 + i)
        else {
            break;
        };
        let display = attrs.contains("display")
            && !attrs.contains("display=\"inline\"")
            && !attrs.contains("display='inline'");
        out.push_str(&html[at..start]);
        out.push_str(&to_html(
            &decode_entities(&html[tag_end + 1..close]),
            display,
        ));
        at = close + "</math>".len();
        changed = true;
    }
    if !changed {
        return None;
    }
    out.push_str(&html[at..]);
    Some(out)
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&#x2f;", "/")
        .replace("&#x5c;", "\\")
        .replace("&amp;", "&")
}

fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}

// ---- the tree -----------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Style {
    /// Letters in math italic (the default for variables).
    Italic,
    Upright,
    Bold,
}

#[derive(Debug, Clone)]
enum Node {
    Row(Vec<Node>),
    /// An ordinary symbol or run of them.
    Ord(String, Style),
    /// A function name (`sin`, `det`): upright, with a thin space after it unless a bracket follows.
    Func(String),
    Bin(String),
    Rel(String),
    Open(String),
    Close(String),
    Punct(String),
    /// A big operator (`∑ ∏ ∫ ...`) or a name set like one (`lim`, `max`); `limits` puts scripts above and below in display.
    Big {
        sym: String,
        limits: bool,
        name: bool,
    },
    Frac(Box<Node>, Box<Node>),
    Sqrt(Option<Box<Node>>, Box<Node>),
    Script {
        base: Box<Node>,
        sub: Option<Box<Node>>,
        sup: Option<Box<Node>>,
    },
    Matrix {
        rows: Vec<Vec<Node>>,
        left: String,
        right: String,
        align_left: bool,
    },
    Accent(&'static str, Box<Node>),
    Over(Box<Node>),
    Under(Box<Node>),
    Boxed(Box<Node>),
    /// Space in em.
    Space(f32),
    LineBreak,
}

// ---- the parser ---------------------------------------------------------------------------------------------

struct Parser {
    s: Vec<char>,
    i: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }
    fn starts_with(&self, text: &str) -> bool {
        text.chars()
            .enumerate()
            .all(|(k, c)| self.s.get(self.i + k) == Some(&c))
    }

    /// Atoms until one of `stops` (not consumed) or the end.
    fn row(&mut self, stops: &[&str]) -> Node {
        let mut items = Vec::new();
        if self.depth >= MAX_DEPTH {
            self.i = self.s.len();
            return Node::Row(items);
        }
        self.depth += 1;
        loop {
            self.skip_ws();
            let Some(c) = self.peek() else { break };
            if stops.iter().any(|s| self.starts_with(s)) {
                break;
            }
            if c == '}' && !stops.contains(&"}") {
                self.i += 1; // a stray closing brace
                continue;
            }
            let atom = self.atom();
            let atom = self.scripts(atom);
            items.push(atom);
        }
        self.depth -= 1;
        Node::Row(items)
    }

    /// `^` and `_` after an atom (and `'` for a prime).
    fn scripts(&mut self, base: Node) -> Node {
        let (mut sub, mut sup) = (None, None);
        loop {
            self.skip_ws();
            match self.peek() {
                Some('^') if sup.is_none() => {
                    self.i += 1;
                    sup = Some(Box::new(self.arg()));
                }
                Some('_') if sub.is_none() => {
                    self.i += 1;
                    sub = Some(Box::new(self.arg()));
                }
                Some('\'') if sup.is_none() => {
                    let mut n = 0;
                    while self.peek() == Some('\'') {
                        self.i += 1;
                        n += 1;
                    }
                    sup = Some(Box::new(Node::Ord("′".repeat(n), Style::Upright)));
                }
                _ => break,
            }
        }
        if sub.is_none() && sup.is_none() {
            base
        } else {
            Node::Script {
                base: Box::new(base),
                sub,
                sup,
            }
        }
    }

    /// One argument: a `{group}` or a single atom.
    fn arg(&mut self) -> Node {
        self.skip_ws();
        if self.peek() == Some('{') {
            self.i += 1;
            let row = self.row(&["}"]);
            if self.peek() == Some('}') {
                self.i += 1;
            }
            row
        } else if self.peek().is_some() {
            self.atom()
        } else {
            Node::Row(Vec::new())
        }
    }

    /// Raw text up to the matching `}` (for `\text{...}`).
    fn raw_group(&mut self) -> String {
        self.skip_ws();
        if self.peek() != Some('{') {
            return self
                .peek()
                .map(|c| {
                    self.i += 1;
                    c.to_string()
                })
                .unwrap_or_default();
        }
        self.i += 1;
        let (mut depth, mut out) = (1, String::new());
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                '{' => {
                    depth += 1;
                    out.push(c)
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(c)
                }
                _ => out.push(c),
            }
        }
        out
    }

    fn atom(&mut self) -> Node {
        let Some(c) = self.peek() else {
            return Node::Row(Vec::new());
        };
        self.i += 1;
        match c {
            '{' => {
                let row = self.row(&["}"]);
                if self.peek() == Some('}') {
                    self.i += 1;
                }
                row
            }
            '\\' => self.command(),
            '0'..='9' | '.' => {
                let mut run = c.to_string();
                while self.peek().is_some_and(|d| d.is_ascii_digit() || d == '.') {
                    run.push(self.peek().unwrap());
                    self.i += 1;
                }
                Node::Ord(run, Style::Upright)
            }
            '+' => Node::Bin("+".into()),
            '-' => Node::Bin("−".into()),
            '*' => Node::Bin("∗".into()),
            '/' => Node::Ord("/".into(), Style::Upright),
            '=' | '<' | '>' => Node::Rel(c.to_string()),
            ',' | ';' => Node::Punct(c.to_string()),
            ':' => Node::Rel(":".into()),
            '(' | '[' => Node::Open(c.to_string()),
            ')' | ']' => Node::Close(c.to_string()),
            '|' => Node::Ord("|".into(), Style::Upright),
            '!' => Node::Ord("!".into(), Style::Upright),
            '~' => Node::Space(0.3),
            c if c.is_alphabetic() => Node::Ord(
                c.to_string(),
                if is_greek(c) || !c.is_ascii() {
                    Style::Upright
                } else {
                    Style::Italic
                },
            ),
            c => Node::Ord(c.to_string(), Style::Upright),
        }
    }

    fn command(&mut self) -> Node {
        let Some(first) = self.peek() else {
            return Node::Ord("\\".into(), Style::Upright);
        };
        if !first.is_ascii_alphabetic() {
            self.i += 1;
            return match first {
                '\\' => Node::LineBreak,
                ',' => Node::Space(0.17),
                ':' => Node::Space(0.22),
                ';' => Node::Space(0.28),
                '!' => Node::Space(-0.17),
                ' ' => Node::Space(0.3),
                '{' | '}' | '%' | '$' | '&' | '#' | '_' => {
                    Node::Ord(first.to_string(), Style::Upright)
                }
                '|' => Node::Ord("‖".into(), Style::Upright),
                _ => Node::Ord(first.to_string(), Style::Upright),
            };
        }
        let mut name = String::new();
        while let Some(c) = self.peek().filter(char::is_ascii_alphabetic) {
            name.push(c);
            self.i += 1;
        }
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let (n, d) = (self.arg(), self.arg());
                Node::Frac(Box::new(n), Box::new(d))
            }
            "binom" | "dbinom" | "tbinom" => {
                let (n, k) = (self.arg(), self.arg());
                Node::Matrix {
                    rows: vec![vec![n], vec![k]],
                    left: "(".into(),
                    right: ")".into(),
                    align_left: false,
                }
            }
            "sqrt" => {
                self.skip_ws();
                let index = if self.peek() == Some('[') {
                    self.i += 1;
                    let r = self.row(&["]"]);
                    if self.peek() == Some(']') {
                        self.i += 1;
                    }
                    Some(Box::new(r))
                } else {
                    None
                };
                Node::Sqrt(index, Box::new(self.arg()))
            }
            "text" | "textrm" | "textit" | "textbf" | "mbox" | "hbox" => {
                Node::Ord(self.raw_group(), Style::Upright)
            }
            "mathrm" | "mathsf" | "mathtt" | "operatorname" | "mathup" => {
                self.styled(Style::Upright)
            }
            "mathbf" | "boldsymbol" | "bm" | "mathbfit" => self.styled(Style::Bold),
            "mathit" => self.styled(Style::Italic),
            "mathbb" => self.mapped(blackboard),
            "mathcal" | "mathscr" => self.mapped(script),
            "left" => {
                let open = self.delimiter();
                let inner = self.row(&["\\right"]);
                let close = if self.starts_with("\\right") {
                    self.i += "\\right".len();
                    self.delimiter()
                } else {
                    String::new()
                };
                Node::Matrix {
                    rows: vec![vec![inner]],
                    left: open,
                    right: close,
                    align_left: false,
                }
            }
            "right" => Node::Row(Vec::new()), // a stray \right
            "begin" => self.environment(),
            "end" => {
                self.raw_group();
                Node::Row(Vec::new())
            }
            "overline" | "bar" if name == "overline" => Node::Over(Box::new(self.arg())),
            "underline" => Node::Under(Box::new(self.arg())),
            "boxed" => Node::Boxed(Box::new(self.arg())),
            "hat" | "widehat" => Node::Accent("^", Box::new(self.arg())),
            "tilde" | "widetilde" => Node::Accent("~", Box::new(self.arg())),
            "bar" => Node::Accent("¯", Box::new(self.arg())),
            "vec" | "overrightarrow" => Node::Accent("→", Box::new(self.arg())),
            "dot" => Node::Accent("˙", Box::new(self.arg())),
            "ddot" => Node::Accent("¨", Box::new(self.arg())),
            "displaystyle" | "textstyle" | "scriptstyle" | "limits" | "nolimits" | "mathrel"
            | "mathbin" | "mathord" => Node::Row(Vec::new()),
            "quad" => Node::Space(1.0),
            "qquad" => Node::Space(2.0),
            "hspace" | "hskip" => {
                self.raw_group();
                Node::Space(1.0)
            }
            "sum" | "prod" | "coprod" | "bigcup" | "bigcap" | "bigoplus" | "bigotimes"
            | "bigvee" | "bigwedge" => {
                let sym = match name.as_str() {
                    "sum" => "∑",
                    "prod" => "∏",
                    "coprod" => "∐",
                    "bigcup" => "⋃",
                    "bigcap" => "⋂",
                    "bigoplus" => "⨁",
                    "bigotimes" => "⨂",
                    "bigvee" => "⋁",
                    _ => "⋀",
                };
                Node::Big {
                    sym: sym.into(),
                    limits: true,
                    name: false,
                }
            }
            "int" | "iint" | "iiint" | "oint" => {
                let sym = match name.as_str() {
                    "int" => "∫",
                    "iint" => "∬",
                    "iiint" => "∭",
                    _ => "∮",
                };
                Node::Big {
                    sym: sym.into(),
                    limits: false,
                    name: false,
                }
            }
            "lim" | "max" | "min" | "sup" | "inf" | "limsup" | "liminf" | "argmax" | "argmin" => {
                Node::Big {
                    sym: name.clone(),
                    limits: true,
                    name: true,
                }
            }
            "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "arcsin" | "arccos" | "arctan"
            | "sinh" | "cosh" | "tanh" | "coth" | "log" | "ln" | "lg" | "exp" | "det" | "gcd"
            | "deg" | "dim" | "ker" | "arg" | "Pr" | "hom" | "mod" | "bmod" => {
                Node::Func(name.clone())
            }
            _ => match symbol(&name) {
                Some((class, text)) => match class {
                    'b' => Node::Bin(text.into()),
                    'r' => Node::Rel(text.into()),
                    'o' => Node::Open(text.into()),
                    'c' => Node::Close(text.into()),
                    'p' => Node::Punct(text.into()),
                    'g' => Node::Ord(text.into(), Style::Upright),
                    _ => Node::Ord(text.into(), Style::Upright),
                },
                None => Node::Ord(format!("\\{name}"), Style::Upright), // unknown: shown as written
            },
        }
    }

    fn delimiter(&mut self) -> String {
        self.skip_ws();
        let Some(c) = self.peek() else {
            return String::new();
        };
        self.i += 1;
        match c {
            '.' => String::new(),
            '\\' => {
                let mut name = String::new();
                if let Some(n) = self.peek().filter(|n| !n.is_ascii_alphabetic()) {
                    self.i += 1;
                    return match n {
                        '{' => "{".into(),
                        '}' => "}".into(),
                        '|' => "‖".into(),
                        _ => n.to_string(),
                    };
                }
                while let Some(c) = self.peek().filter(char::is_ascii_alphabetic) {
                    name.push(c);
                    self.i += 1;
                }
                match name.as_str() {
                    "langle" => "⟨".into(),
                    "rangle" => "⟩".into(),
                    "lfloor" => "⌊".into(),
                    "rfloor" => "⌋".into(),
                    "lceil" => "⌈".into(),
                    "rceil" => "⌉".into(),
                    "lvert" | "rvert" | "vert" => "|".into(),
                    "lVert" | "rVert" | "Vert" => "‖".into(),
                    _ => String::new(),
                }
            }
            c => c.to_string(),
        }
    }

    fn styled(&mut self, style: Style) -> Node {
        let arg = self.arg();
        restyle(arg, style)
    }

    fn mapped(&mut self, f: fn(char) -> Option<char>) -> Node {
        let arg = self.arg();
        map_chars(arg, f)
    }

    fn environment(&mut self) -> Node {
        let env = self.raw_group();
        let (left, right, align_left) = match env.as_str() {
            "pmatrix" => ("(", ")", false),
            "bmatrix" => ("[", "]", false),
            "Bmatrix" => ("{", "}", false),
            "vmatrix" => ("|", "|", false),
            "Vmatrix" => ("‖", "‖", false),
            "cases" => ("{", "", true),
            "matrix" | "smallmatrix" | "array" | "aligned" | "align" | "gathered" | "split"
            | "eqnarray" => (
                "",
                "",
                env.starts_with("align") || env == "split" || env == "eqnarray",
            ),
            _ => ("", "", false),
        };
        if env == "array" {
            self.skip_ws();
            if self.peek() == Some('{') {
                self.raw_group(); // the column spec
            }
        }
        let end = format!("\\end{{{env}}}");
        let mut rows: Vec<Vec<Node>> = vec![Vec::new()];
        loop {
            let cell = self.row(&["&", "\\\\", &end]);
            rows.last_mut().unwrap().push(cell);
            if self.starts_with("&") {
                self.i += 1;
            } else if self.starts_with("\\\\") {
                self.i += 2;
                rows.push(Vec::new());
            } else {
                if self.starts_with(&end) {
                    self.i += end.chars().count();
                }
                break;
            }
        }
        if rows
            .last()
            .is_some_and(|r| r.iter().all(|c| matches!(c, Node::Row(v) if v.is_empty())))
            && rows.len() > 1
        {
            rows.pop(); // a trailing \\
        }
        Node::Matrix {
            rows,
            left: left.into(),
            right: right.into(),
            align_left,
        }
    }
}

fn restyle(n: Node, style: Style) -> Node {
    match n {
        Node::Row(v) => Node::Row(v.into_iter().map(|x| restyle(x, style)).collect()),
        Node::Ord(s, _) => Node::Ord(s, style),
        Node::Script { base, sub, sup } => Node::Script {
            base: Box::new(restyle(*base, style)),
            sub,
            sup,
        },
        other => other,
    }
}

fn map_chars(n: Node, f: fn(char) -> Option<char>) -> Node {
    match n {
        Node::Row(v) => Node::Row(v.into_iter().map(|x| map_chars(x, f)).collect()),
        Node::Ord(s, _) => Node::Ord(
            s.chars().map(|c| f(c).unwrap_or(c)).collect(),
            Style::Upright,
        ),
        Node::Script { base, sub, sup } => Node::Script {
            base: Box::new(map_chars(*base, f)),
            sub,
            sup,
        },
        other => other,
    }
}

fn blackboard(c: char) -> Option<char> {
    Some(match c {
        'C' => 'ℂ',
        'H' => 'ℍ',
        'N' => 'ℕ',
        'P' => 'ℙ',
        'Q' => 'ℚ',
        'R' => 'ℝ',
        'Z' => 'ℤ',
        'A'..='Z' => char::from_u32(0x1D538 + (c as u32 - 'A' as u32))?,
        'a'..='z' => char::from_u32(0x1D552 + (c as u32 - 'a' as u32))?,
        '0'..='9' => char::from_u32(0x1D7D8 + (c as u32 - '0' as u32))?,
        _ => return None,
    })
}

fn script(c: char) -> Option<char> {
    Some(match c {
        'B' => 'ℬ',
        'E' => 'ℰ',
        'F' => 'ℱ',
        'H' => 'ℋ',
        'I' => 'ℐ',
        'L' => 'ℒ',
        'M' => 'ℳ',
        'R' => 'ℛ',
        'e' => 'ℯ',
        'g' => 'ℊ',
        'o' => 'ℴ',
        'A'..='Z' => char::from_u32(0x1D49C + (c as u32 - 'A' as u32))?,
        'a'..='z' => char::from_u32(0x1D4B6 + (c as u32 - 'a' as u32))?,
        _ => return None,
    })
}

fn is_greek(c: char) -> bool {
    ('\u{0370}'..='\u{03FF}').contains(&c)
}

/// `\name` to (class, text). Classes: `b` binary operator, `r` relation, `o` opening, `c` closing, `p` punctuation,
/// anything else an ordinary symbol.
fn symbol(name: &str) -> Option<(char, &'static str)> {
    Some(match name {
        // Greek
        "alpha" => ('g', "α"),
        "beta" => ('g', "β"),
        "gamma" => ('g', "γ"),
        "delta" => ('g', "δ"),
        "epsilon" => ('g', "ϵ"),
        "varepsilon" => ('g', "ε"),
        "zeta" => ('g', "ζ"),
        "eta" => ('g', "η"),
        "theta" => ('g', "θ"),
        "vartheta" => ('g', "ϑ"),
        "iota" => ('g', "ι"),
        "kappa" => ('g', "κ"),
        "lambda" => ('g', "λ"),
        "mu" => ('g', "μ"),
        "nu" => ('g', "ν"),
        "xi" => ('g', "ξ"),
        "pi" => ('g', "π"),
        "varpi" => ('g', "ϖ"),
        "rho" => ('g', "ρ"),
        "varrho" => ('g', "ϱ"),
        "sigma" => ('g', "σ"),
        "varsigma" => ('g', "ς"),
        "tau" => ('g', "τ"),
        "upsilon" => ('g', "υ"),
        "phi" => ('g', "ϕ"),
        "varphi" => ('g', "φ"),
        "chi" => ('g', "χ"),
        "psi" => ('g', "ψ"),
        "omega" => ('g', "ω"),
        "Gamma" => ('g', "Γ"),
        "Delta" => ('g', "Δ"),
        "Theta" => ('g', "Θ"),
        "Lambda" => ('g', "Λ"),
        "Xi" => ('g', "Ξ"),
        "Pi" => ('g', "Π"),
        "Sigma" => ('g', "Σ"),
        "Upsilon" => ('g', "Υ"),
        "Phi" => ('g', "Φ"),
        "Psi" => ('g', "Ψ"),
        "Omega" => ('g', "Ω"),
        // binary operators
        "pm" => ('b', "±"),
        "mp" => ('b', "∓"),
        "times" => ('b', "×"),
        "div" => ('b', "÷"),
        "cdot" => ('b', "⋅"),
        "ast" => ('b', "∗"),
        "star" => ('b', "⋆"),
        "circ" => ('b', "∘"),
        "bullet" => ('b', "•"),
        "oplus" => ('b', "⊕"),
        "ominus" => ('b', "⊖"),
        "otimes" => ('b', "⊗"),
        "odot" => ('b', "⊙"),
        "cup" => ('b', "∪"),
        "cap" => ('b', "∩"),
        "setminus" => ('b', "∖"),
        "land" | "wedge" => ('b', "∧"),
        "lor" | "vee" => ('b', "∨"),
        "uplus" => ('b', "⊎"),
        // relations
        "leq" | "le" => ('r', "≤"),
        "geq" | "ge" => ('r', "≥"),
        "neq" | "ne" => ('r', "≠"),
        "approx" => ('r', "≈"),
        "equiv" => ('r', "≡"),
        "sim" => ('r', "∼"),
        "simeq" => ('r', "≃"),
        "cong" => ('r', "≅"),
        "propto" => ('r', "∝"),
        "ll" => ('r', "≪"),
        "gg" => ('r', "≫"),
        "subset" => ('r', "⊂"),
        "supset" => ('r', "⊃"),
        "subseteq" => ('r', "⊆"),
        "supseteq" => ('r', "⊇"),
        "in" => ('r', "∈"),
        "notin" => ('r', "∉"),
        "ni" => ('r', "∋"),
        "perp" => ('r', "⊥"),
        "parallel" => ('r', "∥"),
        "mid" => ('r', "∣"),
        "to" | "rightarrow" => ('r', "→"),
        "leftarrow" | "gets" => ('r', "←"),
        "leftrightarrow" => ('r', "↔"),
        "Rightarrow" | "implies" => ('r', "⇒"),
        "Leftarrow" => ('r', "⇐"),
        "Leftrightarrow" | "iff" => ('r', "⇔"),
        "mapsto" => ('r', "↦"),
        "uparrow" => ('r', "↑"),
        "downarrow" => ('r', "↓"),
        "longrightarrow" => ('r', "⟶"),
        "longleftarrow" => ('r', "⟵"),
        "approxeq" => ('r', "≊"),
        "doteq" => ('r', "≐"),
        "vdash" => ('r', "⊢"),
        "models" => ('r', "⊨"),
        "prec" => ('r', "≺"),
        "succ" => ('r', "≻"),
        // delimiters
        "langle" => ('o', "⟨"),
        "rangle" => ('c', "⟩"),
        "lfloor" => ('o', "⌊"),
        "rfloor" => ('c', "⌋"),
        "lceil" => ('o', "⌈"),
        "rceil" => ('c', "⌉"),
        "lbrace" => ('o', "{"),
        "rbrace" => ('c', "}"),
        "lbrack" => ('o', "["),
        "rbrack" => ('c', "]"),
        // punctuation and dots
        "ldots" | "dots" | "dotsc" => ('p', "…"),
        "cdots" | "dotsb" => ('p', "⋯"),
        "vdots" => ('g', "⋮"),
        "ddots" => ('g', "⋱"),
        "colon" => ('p', ":"),
        // ordinary symbols
        "infty" => ('g', "∞"),
        "partial" => ('g', "∂"),
        "nabla" => ('g', "∇"),
        "forall" => ('g', "∀"),
        "exists" => ('g', "∃"),
        "nexists" => ('g', "∄"),
        "neg" | "lnot" => ('g', "¬"),
        "emptyset" | "varnothing" => ('g', "∅"),
        "angle" => ('g', "∠"),
        "degree" => ('g', "°"),
        "prime" => ('g', "′"),
        "aleph" => ('g', "ℵ"),
        "ell" => ('g', "ℓ"),
        "hbar" => ('g', "ℏ"),
        "Re" => ('g', "ℜ"),
        "Im" => ('g', "ℑ"),
        "wp" => ('g', "℘"),
        "top" => ('g', "⊤"),
        "bot" => ('g', "⊥"),
        "therefore" => ('g', "∴"),
        "because" => ('g', "∵"),
        "triangle" => ('g', "△"),
        "square" => ('g', "□"),
        "checkmark" => ('g', "✓"),
        "dagger" => ('g', "†"),
        "ddagger" => ('g', "‡"),
        "S" => ('g', "§"),
        "P" => ('g', "¶"),
        "copyright" => ('g', "©"),
        "pounds" => ('g', "£"),
        "euro" => ('g', "€"),
        "peso" => ('g', "₱"),
        "surd" => ('g', "√"),
        "lbar" => ('g', "ƛ"),
        _ => return None,
    })
}

// ---- the HTML -----------------------------------------------------------------------------------------------
//
// A formula is a row of items laid out as a centred flex row (`align-items:center`): runs of text, and boxes (a
// fraction, a root, a matrix, a stack of limits). Every item shares one centre line, which is what puts the bar of a
// fraction level with `=` and `+`. A script is a small box lifted with a transform; a transform moves what is drawn and
// not the layout, so it is only used for things small enough not to disturb their line.

#[derive(Debug, Clone, Default)]
struct Ctx {
    display: bool,
    /// How many times the text has been made smaller (scripts, nested fractions).
    level: u8,
}

/// Roughly how tall a node is, in em of the surrounding text (1.0 is one line).
fn height(n: &Node, display: bool) -> f32 {
    match n {
        Node::Row(v) => v.iter().map(|x| height(x, display)).fold(1.0, f32::max),
        Node::Frac(a, b) => 2.0 * height(a, display).max(height(b, display)) * 1.1 + 0.1,
        Node::Sqrt(_, b) => height(b, display).max(1.0) + 0.15,
        Node::Script { base, sub, sup } => {
            let h = height(base, display);
            h + if sup.is_some() { 0.5 } else { 0.0 } + if sub.is_some() { 0.3 } else { 0.0 }
        }
        Node::Matrix { rows, .. } => {
            let cells = rows
                .iter()
                .flatten()
                .map(|c| height(c, display))
                .fold(1.0, f32::max);
            rows.len() as f32 * 1.25 * cells.max(1.0)
        }
        Node::Big { limits, name, .. } => {
            if *limits && display && !name {
                2.3
            } else {
                1.0
            }
        }
        Node::Accent(..) => 1.3,
        Node::Over(b) | Node::Under(b) | Node::Boxed(b) => height(b, display),
        _ => 1.0,
    }
}

fn small(ctx: &Ctx) -> Ctx {
    Ctx {
        level: ctx.level + 1,
        ..ctx.clone()
    }
}

fn push_text(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

/// A character stretched to `k` times its height (a parenthesis next to a fraction). Empty when `c` is empty.
fn stretched(c: &str, k: f32) -> String {
    if c.is_empty() {
        return "\u{200b}".to_string();
    }
    let mut out = String::new();
    if k <= 1.15 {
        push_text(&mut out, c);
    } else {
        out.push_str(&format!(
            "<span style=\"display:inline-block;transform:scaleY({:.2})\">",
            k.min(4.5)
        ));
        push_text(&mut out, c);
        out.push_str("</span>");
    }
    out
}

fn class_of(n: &Node) -> char {
    match n {
        Node::Bin(_) => 'b',
        Node::Rel(_) => 'r',
        Node::Open(_) => 'o',
        Node::Close(_) => 'c',
        Node::Punct(_) => 'p',
        _ => 'g',
    }
}

/// Is this node laid out as a box of its own (an item of the flex row) rather than as text in a run?
fn is_box(n: &Node, ctx: &Ctx) -> bool {
    match n {
        Node::Frac(..) | Node::Sqrt(..) | Node::Matrix { .. } | Node::Accent(..) => true,
        Node::Script { base, .. } => has_limits_stack(base, ctx) || is_box(base, ctx),
        _ => false,
    }
}

fn has_limits_stack(base: &Node, ctx: &Ctx) -> bool {
    ctx.display && matches!(base, Node::Big { limits: true, .. })
}

fn emit(n: &Node, ctx: &Ctx, out: &mut String) {
    out.push_str(&seq(std::slice::from_ref(n), ctx));
}

/// The nodes of a row, with nested rows (`{...}` groups) opened up.
fn flatten<'a>(nodes: &'a [Node], out: &mut Vec<&'a Node>) {
    for n in nodes {
        match n {
            Node::Row(inner) => flatten(inner, out),
            other => out.push(other),
        }
    }
}

/// A sequence of nodes as HTML: plain inline text when it holds no box, else a centred flex row.
fn seq(nodes: &[Node], ctx: &Ctx) -> String {
    let mut flat: Vec<&Node> = Vec::new();
    flatten(nodes, &mut flat);
    let mut items: Vec<String> = Vec::new();
    let mut run = String::new();
    let mut boxes = 0;
    let mut prev = 'o'; // the start of a row behaves like an opening symbol
    let mut k = 0;
    while k < flat.len() {
        let node = flat[k];
        // Letters in a run share one <i>.
        if let Node::Ord(first, Style::Italic) = node {
            let mut letters = first.clone();
            let mut m = k + 1;
            while let Some(Node::Ord(t, Style::Italic)) = flat.get(m) {
                letters.push_str(t);
                m += 1;
            }
            // A script binds to the last letter only, so stop before a letter that carries one.
            run.push_str("<i>");
            push_text(&mut run, &letters);
            run.push_str("</i>");
            prev = 'g';
            k = m;
            continue;
        }
        match node {
            Node::Bin(s) if matches!(prev, 'b' | 'r' | 'o' | 'p') => push_text(&mut run, s), // unary: −x
            Node::Bin(s) => {
                run.push_str("<span style=\"display:inline-block;padding:0 0.22em\">");
                push_text(&mut run, s);
                run.push_str("</span>");
            }
            Node::Rel(s) => {
                run.push_str("<span style=\"display:inline-block;padding:0 0.28em\">");
                push_text(&mut run, s);
                run.push_str("</span>");
            }
            Node::Punct(s) => {
                push_text(&mut run, s);
                run.push_str(
                    "<span style=\"display:inline-block;padding:0 0.085em\">\u{200b}</span>",
                );
            }
            Node::Ord(s, Style::Bold) => {
                run.push_str("<b>");
                push_text(&mut run, s);
                run.push_str("</b>");
            }
            Node::Func(name) => {
                push_text(&mut run, name);
                if !matches!(flat.get(k + 1), Some(Node::Open(_)) | None) {
                    run.push_str(
                        "<span style=\"display:inline-block;padding:0 0.085em\">\u{200b}</span>",
                    );
                }
            }
            Node::Ord(s, _) | Node::Open(s) | Node::Close(s) => push_text(&mut run, s),
            Node::Space(em) => {
                if *em >= 0.0 {
                    run.push_str(&format!(
                        "<span style=\"display:inline-block;padding:0 {:.3}em\">\u{200b}</span>",
                        em / 2.0
                    ));
                } else {
                    run.push_str(&format!(
                        "<span style=\"display:inline-block;padding:0;margin-left:{em:.2}em\">\u{200b}</span>"
                    ));
                }
            }
            Node::LineBreak => run.push_str("<br>"),
            Node::Big { sym, name, .. } => {
                if *name {
                    push_text(&mut run, sym);
                } else {
                    let size = if ctx.display { 145 } else { 115 };
                    run.push_str(&format!(
                        "<span style=\"display:inline-block;font-size:{size}%\">"
                    ));
                    push_text(&mut run, sym);
                    run.push_str("</span>");
                }
            }
            Node::Over(body) => {
                run.push_str("<span style=\"border-top:1px solid #000;padding-top:0.05em\">");
                run.push_str(&seq(std::slice::from_ref(body), ctx));
                run.push_str("</span>");
            }
            Node::Under(body) => {
                run.push_str("<span style=\"border-bottom:1px solid #000;padding-bottom:0.05em\">");
                run.push_str(&seq(std::slice::from_ref(body), ctx));
                run.push_str("</span>");
            }
            Node::Boxed(body) => {
                run.push_str("<span style=\"display:inline-block;border:1px solid #000;padding:0.2em 0.35em\">");
                run.push_str(&seq(std::slice::from_ref(body), ctx));
                run.push_str("</span>");
            }
            Node::Script { base, sub, sup } => {
                if has_limits_stack(base, ctx) {
                    if !run.is_empty() {
                        items.push(format!("<span>{}</span>", std::mem::take(&mut run)));
                    }
                    items.push(limits_stack(base, sub.as_deref(), sup.as_deref(), ctx));
                    boxes += 1;
                } else {
                    if is_box(base, ctx) {
                        if !run.is_empty() {
                            items.push(format!("<span>{}</span>", std::mem::take(&mut run)));
                        }
                        items.push(box_html(base, ctx));
                        boxes += 1;
                    } else {
                        run.push_str(&seq(std::slice::from_ref(base), ctx));
                    }
                    if let (Some(b), Some(a)) = (sub.as_deref(), sup.as_deref()) {
                        // Both scripts: a stack of two lines, an item of the row so it is centred on the row's line.
                        if !run.is_empty() {
                            items.push(format!("<span>{}</span>", std::mem::take(&mut run)));
                        }
                        items.push(script_stack(base, b, a, ctx));
                        boxes += 1;
                    } else {
                        run.push_str(&scripts(base, sub.as_deref(), sup.as_deref(), ctx));
                    }
                }
            }
            n if is_box(n, ctx) => {
                if !run.is_empty() {
                    items.push(format!("<span>{}</span>", std::mem::take(&mut run)));
                }
                items.push(box_html(n, ctx));
                boxes += 1;
            }
            Node::Row(_)
            | Node::Frac(..)
            | Node::Sqrt(..)
            | Node::Matrix { .. }
            | Node::Accent(..) => {
                // handled by `is_box`; a Row cannot occur after flattening
            }
        }
        prev = class_of(node);
        k += 1;
    }
    if boxes == 0 {
        return run;
    }
    if !run.is_empty() {
        items.push(format!("<span>{run}</span>"));
    }
    // The first item is an empty line of text, so the row's baseline is a text baseline and the row sits on the line.
    format!(
        "<span style=\"display:inline-flex;align-items:center;white-space:nowrap\"><span>\u{200b}</span>{}</span>",
        items.concat()
    )
}

fn box_html(n: &Node, ctx: &Ctx) -> String {
    match n {
        Node::Frac(a, b) => {
            // Two equal-height rows, so the bar between them is the box's centre, which is the line the row is
            // centred on. Inline fractions are a little smaller than display ones.
            let size = if ctx.level == 0 && !ctx.display {
                90
            } else {
                100
            };
            format!(
                "<span style=\"display:inline-grid;grid-template-rows:1fr 1fr;justify-items:stretch;text-align:center;font-size:{size}%;line-height:1.2;margin:0 0.12em\">\
                 <span style=\"padding:0 0.22em 0.05em;border-bottom:1px solid #000;align-self:end\">{}</span>\
                 <span style=\"padding:0.05em 0.22em 0;align-self:start\">{}</span></span>",
                seq(std::slice::from_ref(a), ctx),
                seq(std::slice::from_ref(b), ctx)
            )
        }
        Node::Sqrt(index, body) => {
            let h = height(body, ctx.display);
            let mut out = String::from(
                "<span style=\"display:inline-flex;align-items:center;white-space:nowrap\">",
            );
            if let Some(i) = index {
                out.push_str("<span style=\"display:inline-block;font-size:60%;transform:translateY(-0.7em);margin-right:-0.15em\">");
                out.push_str(&seq(std::slice::from_ref(i), &small(ctx)));
                out.push_str("</span>");
            }
            out.push_str(&format!("<span>{}</span>", stretched("√", h * 1.05)));
            out.push_str("<span style=\"border-top:1px solid #000;padding:0.05em 0.12em 0\">");
            out.push_str(&seq(std::slice::from_ref(body), ctx));
            out.push_str("</span></span>");
            out
        }
        Node::Matrix {
            rows,
            left,
            right,
            align_left,
        } => {
            let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
            let h = height(n, ctx.display);
            let mut out = String::from(
                "<span style=\"display:inline-flex;align-items:center;margin:0 0.1em\">",
            );
            out.push_str(&format!("<span>{}</span>", stretched(left, h * 0.95)));
            out.push_str(&format!(
                "<span style=\"display:inline-grid;grid-template-columns:repeat({cols},auto);gap:0.15em {}em;justify-items:{};padding:0 0.25em;line-height:1.25\">",
                if *align_left { "1.2" } else { "0.9" },
                if *align_left { "start" } else { "center" }
            ));
            for row in rows {
                for c in 0..cols {
                    out.push_str("<span>");
                    match row.get(c) {
                        Some(cell) => out.push_str(&seq(std::slice::from_ref(cell), ctx)),
                        None => out.push('\u{a0}'),
                    }
                    out.push_str("</span>");
                }
            }
            out.push_str("</span>");
            out.push_str(&format!(
                "<span>{}</span></span>",
                stretched(right, h * 0.95)
            ));
            out
        }
        Node::Accent(mark, body) => {
            // The mark above the symbol, and an empty line of the same height below it so the symbol stays on the
            // row's centre line.
            let mut out = String::from(
                "<span style=\"display:inline-flex;flex-direction:column;align-items:center;line-height:1\"><span style=\"font-size:80%;line-height:0.6;height:0.6em\">",
            );
            push_text(&mut out, mark);
            out.push_str("</span><span>");
            out.push_str(&seq(std::slice::from_ref(body), ctx));
            out.push_str("</span><span style=\"line-height:0.6;height:0.6em\"></span></span>");
            out
        }
        other => seq(std::slice::from_ref(other), ctx),
    }
}

/// An operator with its limits above and below (display style): three lines whose middle one, the operator, is the
/// box's centre. For `lim` and friends the name is the middle line.
fn limits_stack(base: &Node, sub: Option<&Node>, sup: Option<&Node>, ctx: &Ctx) -> String {
    let sc = small(ctx);
    let name = matches!(base, Node::Big { name: true, .. });
    let limit = |n: Option<&Node>| match n {
        Some(n) => seq(std::slice::from_ref(n), &sc),
        None => "\u{200b}".to_string(),
    };
    let mut out = String::from(
        "<span style=\"display:inline-grid;grid-template-rows:auto auto auto;justify-items:center;line-height:1;margin:0 0.1em\">",
    );
    out.push_str(&format!(
        "<span style=\"font-size:70%\">{}</span>",
        if name {
            "\u{200b}".to_string()
        } else {
            limit(sup)
        }
    ));
    let mut sym = String::new();
    if let Node::Big { sym: s, .. } = base {
        push_text(&mut sym, s);
    }
    out.push_str(&format!(
        "<span style=\"font-size:{}%\">{sym}</span>",
        if name { 100 } else { 140 }
    ));
    out.push_str(&format!(
        "<span style=\"font-size:70%\">{}</span>",
        limit(sub)
    ));
    out.push_str("</span>");
    out
}

/// The script boxes after a base: a superscript, a subscript, or both stacked.
fn scripts(base: &Node, sub: Option<&Node>, sup: Option<&Node>, ctx: &Ctx) -> String {
    let sc = small(ctx);
    let tall = (height(base, ctx.display) - 1.0).max(0.0);
    let (up, down) = (0.5 + tall * 0.5, 0.28 + tall * 0.3);
    // An operator like ∫ keeps its limits at the side, a little away from the glyph.
    let gap = if matches!(base, Node::Big { .. }) {
        0.2
    } else {
        0.05
    };
    let mut out = String::new();
    match (sub, sup) {
        (None, Some(a)) => {
            out.push_str(&format!("<span style=\"display:inline-block;font-size:70%;transform:translateY(-{up:.2}em);margin-left:{gap:.2}em\">"));
            out.push_str(&seq(std::slice::from_ref(a), &sc));
            out.push_str("</span>");
        }
        (Some(b), None) => {
            out.push_str(&format!("<span style=\"display:inline-block;font-size:70%;transform:translateY({down:.2}em);margin-left:{gap:.2}em\">"));
            out.push_str(&seq(std::slice::from_ref(b), &sc));
            out.push_str("</span>");
        }
        (Some(_), Some(_)) | (None, None) => {}
    }
    out
}

/// A superscript above a subscript, as one narrow box: two lines of the smaller text, `up + down` apart, centred on the
/// row's line (the middle of the two is about 0.11 above the baseline, the row's line about 0.3, so it is moved down).
fn script_stack(base: &Node, sub: &Node, sup: &Node, ctx: &Ctx) -> String {
    let sc = small(ctx);
    let tall = (height(base, ctx.display) - 1.0).max(0.0);
    let (up, down) = (0.5 + tall * 0.5, 0.28 + tall * 0.3);
    let gap = if matches!(base, Node::Big { .. }) {
        0.25
    } else {
        0.1
    };
    format!(
        "<span style=\"display:inline-flex;flex-direction:column;align-items:flex-start;font-size:70%;line-height:1;transform:translateY({:.2}em);margin-left:{gap:.2}em\"><span style=\"padding-bottom:{:.2}em\">{}</span><span>{}</span></span>",
        (0.3 - (up - down) / 2.0) / 0.7,
        ((up + down) - 0.7).max(0.0) / 0.7,
        seq(std::slice::from_ref(sup), &sc),
        seq(std::slice::from_ref(sub), &sc)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(s: &str) -> String {
        to_html(s, false)
    }

    #[test]
    fn letters_numbers_and_operators() {
        let h = html("E = mc^2");
        assert!(h.contains("<i>E</i>") && h.contains("<i>c</i>"), "{h}");
        assert!(h.contains('2') && h.contains("translateY(-"), "{h}");
        assert!(html("a + b").contains("padding:0 0.22em"));
        // Minus at the start is a sign, not a spaced operator.
        assert!(!html("-x").contains("padding:0 0.22em"));
    }

    #[test]
    fn fractions_roots_and_scripts() {
        let h = html("\\frac{a+b}{2}");
        assert!(
            h.contains("grid-template-rows:1fr 1fr") && h.contains("border-bottom:1px solid"),
            "{h}"
        );
        let h = html("\\sqrt{x^2+1}");
        assert!(h.contains('√') && h.contains("border-top:1px solid"), "{h}");
        assert!(html("\\sqrt[3]{x}").contains("font-size:60%"));
        let both = html("x_i^2");
        assert!(
            both.contains("flex-direction:column") && both.contains("<i>i</i>"),
            "{both}"
        );
    }

    #[test]
    fn big_operators_and_limits() {
        let inline = html("\\sum_{k=1}^{n} k");
        assert!(
            inline.contains('∑') && !inline.contains("grid-template-rows:auto auto auto"),
            "{inline}"
        );
        let display = to_html("\\sum_{k=1}^{n} k", true);
        assert!(
            display.contains("grid-template-rows:auto auto auto")
                && display.contains("text-align:center"),
            "{display}"
        );
        // An integral keeps its limits at the side, as in print.
        assert!(!to_html("\\int_0^1 f", true).contains("grid-template-rows:auto auto auto"));
        assert!(html("\\lim_{x\\to 0} f").contains("lim"));
    }

    #[test]
    fn greek_symbols_and_text() {
        let h = html("\\alpha \\leq \\Omega \\infty \\text{ if } x \\in \\mathbb{R}");
        for needle in ["α", "≤", "Ω", "∞", " if ", "∈", "ℝ"] {
            assert!(h.contains(needle), "{needle} in {h}");
        }
        assert!(html("\\mathbf{v}").contains("<b>v</b>"));
        assert!(html("\\mathbb{N}").contains('ℕ'));
    }

    #[test]
    fn matrices_cases_and_delimiters() {
        let m = html("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}");
        assert!(
            m.contains("grid-template-columns:repeat(2,auto)") && m.contains("scaleY"),
            "{m}"
        );
        assert_eq!(m.matches("<span>").count(), 4 + 2 + 1, "{m}"); // four cells, the two delimiter boxes and the row's strut
        let c = html("f(x)=\\begin{cases} 1 & x>0 \\\\ 0 & x\\le 0 \\end{cases}");
        assert!(c.contains('{') && c.contains("justify-items:start"), "{c}");
        assert!(html("\\left( \\frac{a}{b} \\right)").contains("scaleY"));
    }

    #[test]
    fn accents_and_bars() {
        assert!(html("\\hat{x}").contains('^') && html("\\vec{v}").contains('→'));
        assert!(html("\\overline{AB}").contains("border-top"));
    }

    #[test]
    fn unknown_commands_and_bad_input_do_not_fail() {
        assert!(html("\\nosuchthing + 1").contains("\\nosuchthing"));
        assert!(!html("{{{ a").is_empty());
        assert!(!html("a } b").is_empty());
        assert!(!html("\\frac{1}").is_empty());
        assert!(!html("^ _ ^").is_empty());
        assert!(!html("\\begin{pmatrix} a & ").is_empty());
        // Deep nesting is cut, not a stack overflow.
        let deep = format!("{}x{}", "{".repeat(5000), "}".repeat(5000));
        assert!(!html(&deep).is_empty());
        // A huge formula is shown as text.
        let big = "x".repeat(MAX_FORMULA_CHARS + 1);
        assert_eq!(html(&big), big);
    }

    #[test]
    fn the_text_is_escaped() {
        let h = html("a < b \\text{<script>}");
        assert!(!h.contains("<script>") && h.contains("&lt;"), "{h}");
    }

    #[test]
    fn math_tags_in_a_document() {
        let out = expand("<p>Energy <math>E=mc^2</math> and</p><math display>\\frac{1}{2}</math>")
            .unwrap();
        assert!(
            !out.contains("<math") && out.contains("<i>E</i>") && out.contains("text-align:center"),
            "{out}"
        );
        // Entities left by the template's escaping are read back: `a &lt; b` is `a < b`.
        let out = expand("<math>a &lt; b</math>").unwrap();
        assert!(out.contains("padding:0 0.28em"), "{out}");
        assert_eq!(expand("<p>no math</p>"), None);
        assert_eq!(expand("<mathematics>x</mathematics>"), None);
        // An unclosed tag is left alone.
        assert_eq!(expand("<math>x"), None);
    }
}
