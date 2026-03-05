use handlebars::Handlebars;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::io::Write;
use std::process::{Command, Stdio};

// Keep the engine warm in the utils file
static HB: Lazy<Handlebars> = Lazy::new(|| Handlebars::new());

pub fn html_to_pdf<T: Serialize>(
    template_str: &str,
    data: &T,
    width: &str,
    height: &str,
    progress: impl Fn(f32),
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    progress(10.0);
    let html_content = HB.render_template(template_str, data)?;
    progress(40.0);

    let size_css = format!("@page {{ size: {} {}; margin: 0; }}", width, height);

    // Using the "Essential" flags that worked for your installation
    let mut child = Command::new("prince")
        .arg("-")
        .arg("-o")
        .arg("-")
        .arg("--style")
        .arg(format!("data:text/css,{}", size_css))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    progress(60.0);

    let mut stdin = child.stdin.take().ok_or("Failed to open stdin")?;
    stdin.write_all(html_content.as_bytes())?;
    drop(stdin);

    progress(80.0);

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        return Err(format!("PrinceXML Error: {}", err_msg).into());
    }

    progress(100.0);
    Ok(output.stdout)
}
