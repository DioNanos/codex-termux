"""Prepare build-produced native libraries with the existing platform policy."""

load("@rules_python//python:py_runtime_info.bzl", "PyRuntimeInfo")
load(":windows_native.bzl", "WindowsBuildToolsInfo")

def _native_runtime_impl(ctx):
    python = ctx.attr._python[PyRuntimeInfo]
    prefix = ctx.attr.prefix[DefaultInfo].files.to_list()[0]
    receipt = ctx.attr.prefix[OutputGroupInfo].receipt.to_list()[0]
    output = ctx.actions.declare_directory(ctx.label.name)
    sdk = ctx.actions.declare_directory(ctx.label.name + "_sdk")
    if ctx.attr.windows_tools:
        tools = ctx.attr.windows_tools[WindowsBuildToolsInfo]
        if tools.inputs["target"] != ctx.attr.target:
            fail("Windows runtime target must match its declared native tools")
        config = ctx.actions.declare_file(ctx.label.name + ".json")
        ctx.actions.write(config, json.encode({
            "inputs": tools.inputs,
            "manifest": tools.manifest.path,
            "installed_files": [file.path for file in tools.installed_files],
            "prefix": prefix.path,
            "receipt": receipt.path,
            "status": ctx.info_file.path,
            "output": output.path,
            "sdk": sdk.path,
        }))
        ctx.actions.run(
            executable = tools.python.interpreter,
            arguments = [ctx.file._windows_driver.path, "prepare", config.path],
            inputs = depset([config, prefix, receipt, ctx.info_file, ctx.file._driver, ctx.file._windows_driver] + ctx.files._preparers, transitive = [tools.files]),
            outputs = [output, sdk],
            env = tools.environment,
            execution_requirements = {"no-remote-exec": "1"},
            mnemonic = "VoiceWindowsRuntime",
        )
        return [DefaultInfo(files = depset([output])), OutputGroupInfo(sdk = depset([sdk]))]
    ctx.actions.run(
        executable = python.interpreter,
        arguments = [
            ctx.file._driver.path,
            "--prefix",
            prefix.path,
            "--build-receipt",
            receipt.path,
            "--status",
            ctx.info_file.path,
            "--target",
            ctx.attr.target,
            "--output",
            output.path,
            "--sdk-output",
            sdk.path,
        ],
        inputs = depset(
            [prefix, receipt, ctx.info_file, ctx.file._driver, python.interpreter] + ctx.files._preparers,
            transitive = [python.files],
        ),
        outputs = [output, sdk],
        env = {"PATH": "/usr/bin:/bin", "LC_ALL": "C"},
        execution_requirements = {"no-remote-exec": "1", "no-remote-cache": "1"} if ctx.attr.target.endswith("apple-darwin") else {},
        mnemonic = "VoiceNativeRuntime",
        progress_message = "Preparing private voice runtime for " + ctx.attr.target,
    )
    return [
        DefaultInfo(files = depset([output])),
        OutputGroupInfo(sdk = depset([sdk])),
    ]

native_runtime = rule(
    implementation = _native_runtime_impl,
    attrs = {
        "prefix": attr.label(mandatory = True, allow_single_file = True),
        "target": attr.string(mandatory = True),
        "windows_tools": attr.label(cfg = "exec", providers = [WindowsBuildToolsInfo]),
        "_windows_driver": attr.label(default = "//third_party/voice:bazel_windows.py", allow_single_file = True),
        "_driver": attr.label(default = "//third_party/voice:prepare_built_runtime.py", allow_single_file = True),
        "_preparers": attr.label(default = "//third_party/voice:build_inputs"),
        "_python": attr.label(default = "@python_3_12//:py3_runtime", cfg = "exec"),
    },
)
