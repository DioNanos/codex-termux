use crate::BrowserUseRequirementsToml;
use crate::ConfigRequirementsToml;
use pretty_assertions::assert_eq;

#[test]
fn webmcp_requirements_preserve_explicit_values_and_omission() {
    for (contents, expected_browser_use, expected_empty) in [
        ("", None, true),
        ("[browser_use]", Some(None), true),
        (
            "[browser_use]\nallow_webmcp = true",
            Some(Some(true)),
            false,
        ),
        (
            "[browser_use]\nallow_webmcp = false",
            Some(Some(false)),
            false,
        ),
    ] {
        let requirements: ConfigRequirementsToml =
            toml::from_str(contents).expect("parse managed WebMCP policy");
        assert_eq!(
            requirements,
            ConfigRequirementsToml {
                browser_use: expected_browser_use.map(|allow_webmcp| BrowserUseRequirementsToml {
                    allow_webmcp,
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
        assert_eq!(requirements.is_empty(), expected_empty, "{contents}");
    }
}

#[test]
fn webmcp_requirements_reject_non_booleans() {
    for value in ["\"true\"", "1", "[]"] {
        let contents = format!("[browser_use]\nallow_webmcp = {value}");
        let error = toml::from_str::<ConfigRequirementsToml>(&contents)
            .expect_err("WebMCP policy must be a boolean");
        assert!(error.to_string().contains("allow_webmcp"));
    }
}
