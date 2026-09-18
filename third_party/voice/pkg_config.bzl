"""Build the pinned pkg-config with native Mac configure checks."""

load("@rules_foreign_cc//foreign_cc:defs.bzl", "configure_make_variant")

def pkg_config(name, **kwargs):
    """Keep Linux remote builds and select native Mac producers by OS and CPU."""
    tags = kwargs.pop("tags", [])
    configure_make_variant(
        name = name + "_default",
        tags = tags,
        **kwargs
    )
    for arch in ["aarch64", "x86_64"]:
        native.config_setting(
            name = name + "_macos_" + arch,
            constraint_values = ["@platforms//os:macos", "@platforms//cpu:" + arch],
        )
        configure_make_variant(
            name = name + "_native_macos_" + arch,
            exec_compatible_with = ["@platforms//os:macos", "@platforms//cpu:" + arch],
            tags = tags + ["no-remote-exec"],
            **kwargs
        )
    native.alias(
        name = name,
        tags = tags,
        actual = select({
            ":" + name + "_macos_aarch64": ":" + name + "_native_macos_aarch64",
            ":" + name + "_macos_x86_64": ":" + name + "_native_macos_x86_64",
            "//conditions:default": ":" + name + "_default",
        }),
        visibility = kwargs.get("visibility"),
    )
