//! The Rust engine and its JavaScript packages (`.node/desktop/` over IPC, `.node/wasm/` in the browser) must
//! agree. These checks fail the build when one side changes without the other.

use pdf_engine::{error::AppError, ipc::PROTOCOL_VERSION};
use std::{collections::BTreeSet, fs};

fn node_file(name: &str) -> String {
    package_file(".node/desktop", name)
}

fn package_file(package: &str, name: &str) -> String {
    fs::read_to_string(format!("{}/{package}/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// The JavaScript packages that report the engine's errors, and the file that produces their own.
const PACKAGES: [(&str, &str); 2] = [(".node/desktop", "index.js"), (".node/wasm", "index.js")];

/// The quoted names in `export type <name> = 'A' | 'B' ...;`.
fn union_members(source: &str, name: &str) -> BTreeSet<String> {
    let start = source
        .find(&format!("export type {name} ="))
        .unwrap_or_else(|| panic!(".node/desktop/index.d.ts has no `export type {name}`"));
    let body = &source[start..start + source[start..].find(';').unwrap()];
    body.split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

#[test]
fn typescript_error_codes_are_exactly_the_engines() {
    let rust: BTreeSet<String> = AppError::CODES.iter().map(|c| c.to_string()).collect();
    for (package, _) in PACKAGES {
        let ts = union_members(&package_file(package, "index.d.ts"), "ErrorCode");
        assert_eq!(
            ts, rust,
            "{package}/index.d.ts `ErrorCode` and AppError::CODES (src/error.rs) differ"
        );
    }
}

#[test]
fn client_only_codes_do_not_collide_with_engine_codes() {
    for (package, js_file) in PACKAGES {
        let client = union_members(&package_file(package, "index.d.ts"), "ClientErrorCode");
        for code in AppError::CODES {
            assert!(
                !client.contains(*code),
                "{package}: {code} is in both code lists"
            );
        }
        // Every client code is actually produced by the client.
        let js = package_file(package, js_file);
        for code in &client {
            assert!(
                js.contains(&format!("'{code}'")),
                "{package}: {code} is never produced by {js_file}"
            );
        }
    }
}

#[test]
fn protocol_versions_match() {
    let js = node_file("index.js");
    assert!(
        js.contains(&format!("const PROTOCOL_VERSION = {PROTOCOL_VERSION};")),
        ".node/desktop/index.js PROTOCOL_VERSION differs from src/ipc.rs ({PROTOCOL_VERSION})"
    );
    assert!(
        node_file("index.d.ts").contains(&format!("PROTOCOL_VERSION: {PROTOCOL_VERSION};")),
        ".node/desktop/index.d.ts PROTOCOL_VERSION differs from src/ipc.rs ({PROTOCOL_VERSION})"
    );
}

#[test]
fn package_versions_match_cargo() {
    for package in [".node/desktop", ".node/wasm"] {
        let json: serde_json::Value =
            serde_json::from_str(&package_file(package, "package.json")).unwrap();
        assert_eq!(
            json["version"],
            env!("CARGO_PKG_VERSION"),
            "{package}/package.json and Cargo.toml versions differ"
        );
    }
    let wasm_crate = package_file(".node/wasm", "rust/Cargo.toml");
    assert!(
        wasm_crate.contains(&format!("version = \"{}\"", env!("CARGO_PKG_VERSION"))),
        ".node/wasm/rust/Cargo.toml version differs from Cargo.toml"
    );
}

#[test]
fn every_engine_error_the_client_can_receive_has_a_listed_code() {
    // Each variant's code must be in CODES, so a new variant cannot ship unlisted.
    let samples = [
        AppError::Unauthorized,
        AppError::MissingField("x".into()),
        AppError::InvalidField("x".into()),
        AppError::Protocol("x".into()),
        AppError::Font("x".into()),
        AppError::PayloadTooLarge(1),
        AppError::TemplateOutputTooLarge(1),
        AppError::TemplateTooExpensive,
        AppError::StorageNotConfigured,
        AppError::Render("x".into()),
        AppError::PdfProcessing("x".into()),
        AppError::Timeout(std::time::Duration::from_secs(1)),
    ];
    for e in samples {
        assert!(
            AppError::CODES.contains(&e.code()),
            "{} is not in CODES",
            e.code()
        );
    }
}
