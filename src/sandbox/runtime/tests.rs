// SPDX-License-Identifier: MIT

use std::{
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStringExt,
    path::Path,
};

use super::Runtime;

const VARIABLES: [&str; 4] = [
    "STRATA_SANDBOX_PATH",
    "STRATA_SANDBOX_ROOT",
    "STRATA_SANDBOX_PRLIMIT",
    "STRATA_SANDBOX_GDK_PIXBUF_MODULE_FILE",
];

fn read_values(name: &str, values: &[Option<&str>; 4]) -> Option<OsString> {
    VARIABLES
        .iter()
        .position(|variable| *variable == name)
        .and_then(|index| values[index].map(OsString::from))
}

fn resolved_values(runtime: &Runtime) -> [Option<&OsStr>; 4] {
    [
        Some(runtime.path.as_os_str()),
        Some(runtime.root.as_os_str()),
        Some(runtime.prlimit.as_os_str()),
        runtime.gdk_pixbuf_module_file.as_deref(),
    ]
}

#[test]
fn runtime_values_take_precedence_over_embedded_values_and_defaults() {
    let defaults = [
        Some("/usr/bin"),
        Some("/usr"),
        Some("/usr/bin/prlimit"),
        None,
    ];
    let embedded = [
        Some("/opt/runtime/bin"),
        Some("/opt/runtime"),
        Some("/opt/runtime/bin/prlimit"),
        Some("/opt/runtime/loaders.cache"),
    ];
    let launched = [
        Some("/nix/store/tools/bin"),
        Some("/nix/store"),
        Some("/nix/store/limits/bin/prlimit"),
        Some("/nix/store/pixbuf/loaders.cache"),
    ];
    for (runtime_values, embedded_values, expected) in [
        ([None; 4], [None; 4], defaults),
        ([None; 4], embedded, embedded),
        (launched, [None; 4], launched),
        (launched, embedded, launched),
        (
            [Some(""), Some(""), Some(""), Some("")],
            embedded,
            [Some(""), embedded[1], Some(""), Some("")],
        ),
    ] {
        let runtime = Runtime::resolve_with(
            |name| read_values(name, &runtime_values),
            |name| read_values(name, &embedded_values),
        );
        assert_eq!(
            resolved_values(&runtime),
            expected.map(|value| value.map(OsStr::new))
        );
    }
}

#[test]
fn runtime_root_accepts_only_the_exact_system_trees() {
    for root in ["/usr", "/nix/store", "/gnu/store"] {
        let runtime = Runtime::resolve_with(
            |name| (name == "STRATA_SANDBOX_ROOT").then(|| OsString::from(root)),
            |_| Some(OsString::from("/opt/embedded-runtime")),
        );
        assert_eq!(runtime.root, OsStr::new(root));
    }
}

#[test]
fn unsafe_runtime_roots_retain_the_embedded_or_default_read_only_mount() {
    for root in [
        "",
        "/",
        "/home/alice",
        "/tmp/runtime",
        "usr",
        "/usr/",
        "//usr",
        "/usr/.",
        "/usr/../home",
        "/nix/store/../../home",
        "/nix/store-extra",
        "/nix/store/package",
        "/gnu/store/../..",
        "/gnu//store",
    ] {
        for embedded in [None, Some("/opt/embedded-runtime")] {
            let runtime = Runtime::resolve_with(
                |name| (name == "STRATA_SANDBOX_ROOT").then(|| OsString::from(root)),
                |name| {
                    (name == "STRATA_SANDBOX_ROOT")
                        .then(|| embedded.map(OsString::from))
                        .flatten()
                },
            );
            let expected = OsStr::new(embedded.unwrap_or("/usr"));
            assert_eq!(runtime.root, expected, "runtime root: {root:?}");
        }
    }
}

#[test]
fn operator_controlled_values_preserve_non_utf8_and_shell_metacharacters() {
    let literal = OsString::from_vec(b"/runtime path/'$tools;\xff".to_vec());
    let runtime = Runtime::resolve_with(
        |_| Some(literal.clone()),
        |_| Some(OsString::from("/embedded/fallback")),
    );
    assert_eq!(runtime.path, literal);
    assert_eq!(runtime.prlimit, literal);
    assert_eq!(runtime.gdk_pixbuf_module_file.as_ref(), Some(&literal));
    let command = crate::sandbox::runtime_command(Path::new("/usr/bin/bwrap"), false, &runtime);
    let arguments: Vec<_> = command.get_args().collect();
    for name in ["PATH", "GDK_PIXBUF_MODULE_FILE"] {
        assert!(arguments.windows(3).any(|args| {
            args == [
                OsStr::new("--setenv"),
                OsStr::new(name),
                literal.as_os_str(),
            ]
        }));
    }
}
