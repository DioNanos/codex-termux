"""Expose prepared native libraries to standard CcInfo linking and runfiles."""

load("@bazel_tools//tools/cpp:toolchain_utils.bzl", "find_cpp_toolchain")
load("@rules_cc//cc/common:cc_common.bzl", "cc_common")
load("@rules_cc//cc/common:cc_info.bzl", "CcInfo")
load("@rules_python//python:py_runtime_info.bzl", "PyRuntimeInfo")

# ABI filenames from the pinned native sources. Missing outputs fail the build.
_ABI_VERSIONS = {
    "ffi": "8",
    "gio-2.0": "0",
    "glib-2.0": "0",
    "gmodule-2.0": "0",
    "gobject-2.0": "0",
    "gstapp-1.0": "0",
    "gstaudio-1.0": "0",
    "gstbase-1.0": "0",
    "gstnet-1.0": "0",
    "gstpbutils-1.0": "0",
    "gstreamer-1.0": "0",
    "gstrtp-1.0": "0",
    "gsttag-1.0": "0",
    "gstvideo-1.0": "0",
    "intl": "8",
    "opus": "0",
    "pcre2-8": "0",
    "z": "1",
}

def _native_link_impl(ctx):
    runtime = ctx.file.runtime
    versions = dict(_ABI_VERSIONS)
    macos = ctx.attr.target.endswith("apple-darwin")
    windows = ctx.attr.target.endswith("windows-msvc")
    if ctx.attr.target.endswith("unknown-linux-gnu"):
        versions["gstallocators-1.0"] = "0"
    sdk = ctx.attr.runtime[OutputGroupInfo].sdk.to_list()[0] if windows else None
    locator = ctx.actions.declare_directory(ctx.label.name + "/lib/search-path")
    originals, aliases, libraries = [], [], []
    arguments = [locator.path]
    cc = find_cpp_toolchain(ctx)
    features = cc_common.configure_features(
        ctx = ctx,
        cc_toolchain = cc,
        requested_features = ctx.features,
        unsupported_features = ctx.disabled_features,
    )
    for name, version in versions.items():
        filename = "lib" + name + ("." + version + ".dylib" if macos else ".so." + version)
        alias = "lib" + name + (".dylib" if macos else ".so")
        directory = "lib"
        if windows:
            # Windows names come from the same pinned recipe's SDK and runtime.
            filename = {"ffi": "libffi-8.dll", "intl": "intl-8.dll", "opus": "opus.dll", "pcre2-8": "pcre2-8.dll", "z": "z.dll"}.get(name, name + "-0.dll")
            alias = ("libffi" if name == "ffi" else name) + ".lib"
            directory = "bin"
        library = ctx.actions.declare_file(ctx.label.name + "/" + directory + "/" + filename)
        development = ctx.actions.declare_file(ctx.label.name + "/lib/" + alias)
        originals.append(library)
        aliases.append(development)
        arguments.extend([runtime.path + "/" + directory + "/" + filename, library.path])
        arguments.extend([sdk.path + "/lib/" + alias if windows else runtime.path + "/lib/" + filename, development.path])
        libraries.append(cc_common.create_library_to_link(
            actions = ctx.actions,
            feature_configuration = features,
            cc_toolchain = cc,
            dynamic_library = library,
            interface_library = development if windows else None,
            # @loader_path/$ORIGIN dependencies must stay beside one another.
            dynamic_library_symlink_path = "voice/" + ctx.label.name + "/" + filename,
        ))
    payloads = []
    paths = ["runtime.json"] + [
        ("bin/gst" + plugin + ".dll" if windows else "plugins/libgst" + plugin + ".dylib" if macos else "lib/gstreamer-1.0/libgst" + plugin + ".so")
        for plugin in "app audioconvert audioresample coreelements opus rtp rtpmanager".split(" ")
    ]
    for path in paths:
        payload = ctx.actions.declare_file(ctx.label.name + "/" + path)
        payloads.append(payload)
        arguments.extend([runtime.path + "/" + path, payload.path])
    python = ctx.attr._python[PyRuntimeInfo]
    ctx.actions.run(
        executable = python.interpreter,
        inputs = depset([runtime, ctx.file._copy, python.interpreter] + ([sdk] if windows else []), transitive = [python.files]),
        outputs = [locator] + originals + aliases + payloads,
        arguments = [ctx.file._copy.path] + arguments,
        mnemonic = "VoiceNativeLinkInputs",
    )
    linker = cc_common.create_linker_input(
        owner = ctx.label,
        user_link_flags = depset([] if windows else [
            "-Wl,-rpath," + ("@loader_path/../lib" if macos else "$ORIGIN/../lib"),
        ]),
        libraries = depset(libraries),
        additional_inputs = depset([locator] + aliases),
    )
    return [
        CcInfo(linking_context = cc_common.create_linking_context(linker_inputs = depset([linker]))),
        DefaultInfo(
            # A real directory permits unambiguous $(execpath :native_link)/..
            # in build-script settings; a marker file followed by /.. would fail.
            files = depset([locator]),
            runfiles = ctx.runfiles(files = [locator] + originals + aliases + payloads + [lib.dynamic_library for lib in libraries]),
        ),
    ]

native_link = rule(
    implementation = _native_link_impl,
    attrs = {
        "runtime": attr.label(mandatory = True, allow_single_file = True),
        "target": attr.string(mandatory = True),
        "_copy": attr.label(default = "//third_party/voice:bazel_copy.py", allow_single_file = True),
        "_python": attr.label(default = "@python_3_12//:py3_runtime", cfg = "exec"),
    },
    fragments = ["cpp"],
    toolchains = ["@bazel_tools//tools/cpp:toolchain_type"],
)
