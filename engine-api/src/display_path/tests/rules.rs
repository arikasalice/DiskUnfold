//! The display rule removes the prefix only when the plain form names the same path.
use crate::display_path::windows;

/// Restores the extended form of a plain drive or UNC path.
fn extended(plain: &str) -> String {
    match plain.strip_prefix(r"\\") {
        Some(network) => format!(r"\\?\UNC\{network}"),
        None => format!(r"\\?\{plain}"),
    }
}

fn kept(input: &str) {
    assert_eq!(windows(input), input, "{input}");
}

fn shown(input: &str, expected: &str) {
    assert_eq!(windows(input), expected, "{input}");
    assert_eq!(extended(expected), input, "{input}");
}

#[test]
fn trailing_dots_spaces_and_relative_components_keep_the_prefix() {
    for input in [
        r"\\?\C:\trailing.",
        r"\\?\C:\trailing.\file",
        r"\\?\C:\space ",
        r"\\?\C:\space \file",
        r"\\?\C:\ ",
        r"\\?\C:\a\file.",
        r"\\?\C:\a\...",
        r"\\?\C:\.",
        r"\\?\C:\a\.\b",
        r"\\?\C:\a\..",
        r"\\?\C:\a\..\b",
        r"\\?\UNC\server\share\trailing.",
        r"\\?\UNC\server\share \x",
        r"\\?\UNC\server.\share",
        r"\\?\UNC\server\share\..",
    ] {
        kept(input);
    }
}

#[test]
fn reserved_device_names_keep_the_prefix_in_any_component_and_case() {
    for name in [
        "CON",
        "prn",
        "Aux",
        "nul",
        "COM0",
        "com1",
        "COM9",
        "LPT0",
        "LPT1",
        "lpt9",
        "COM¹",
        "com²",
        "LPT³",
        "CONIN$",
        "conout$",
        "NUL.txt",
        "con.tar.gz",
        "NUL .txt",
        "nul ",
        "COM1.log",
        "Lpt².x",
        " CON",
        "AUX  .md",
    ] {
        for input in [
            format!(r"\\?\C:\{name}"),
            format!(r"\\?\C:\{name}\x"),
            format!(r"\\?\C:\a\{name}"),
            format!(r"\\?\UNC\server\share\{name}"),
            format!(r"\\?\UNC\{name}\share"),
        ] {
            kept(&input);
        }
    }
}

#[test]
fn names_that_only_resemble_devices_lose_the_prefix() {
    for name in [
        "CONSOLE",
        "NULL",
        "COM10",
        "LPT",
        "COM",
        "COMA",
        "xCON",
        "CON_x",
        "AUX1",
        "CONIN",
        "COM¹0",
        "COM⁴",
        "nul-file.txt",
        "a.NUL",
        ".con",
    ] {
        shown(&format!(r"\\?\C:\{name}"), &format!(r"C:\{name}"));
    }
}

#[test]
fn separators_special_characters_and_empty_components_keep_the_prefix() {
    for input in [
        r"\\?\C:\mixed/slash\x",
        r"\\?\C:\a//b",
        r"\\?\C:/x",
        r"\\?\C:\a\\b",
        r"\\?\C:\\",
        r"\\?\C:\a\\",
        r"\\?\C:\a:stream",
        r"\\?\C:\what?",
        r"\\?\C:\star*",
        r"\\?\C:\<x>",
        r#"\\?\C:\q"t"#,
        r"\\?\C:\pipe|x",
        "\\\\?\\C:\\tab\tx",
        "\\\\?\\C:\\ctl\u{1}x",
        r"\\?\C:",
        r"\\?\C:relative",
        r"\\?\UNC\server\\share",
        r"\\?\UNC\server\share\\x",
        r"\\?\UNC\?\C:\x",
        r"\\?\UNC\server/x\share",
    ] {
        kept(input);
    }
}

#[test]
fn one_trailing_separator_is_allowed() {
    shown(r"\\?\C:\", r"C:\");
    shown(r"\\?\C:\a\", r"C:\a\");
    shown(r"\\?\UNC\server\share\", r"\\server\share\");
}

#[test]
fn the_plain_form_stays_below_max_path_in_utf16_units() {
    let drive = |n: usize| format!(r"\\?\C:\{}", "a".repeat(n));
    shown(&drive(256), &format!(r"C:\{}", "a".repeat(256)));
    kept(&drive(257));
    kept(&drive(300));
    // Each emoji is one character, two UTF-16 units and four UTF-8 bytes.
    let emoji = "😀".repeat(128);
    shown(&format!(r"\\?\C:\{emoji}"), &format!(r"C:\{emoji}"));
    kept(&format!(r"\\?\C:\{emoji}a"));
    // UNC paths are measured after conversion: "\\s\h\" is six units.
    let unc = |n: usize| format!(r"\\?\UNC\s\h\{}", "b".repeat(n));
    shown(&unc(253), &format!(r"\\s\h\{}", "b".repeat(253)));
    kept(&unc(254));
}

/// Components with a known answer: may Win32 read them without the prefix?
const NAMES: [(&str, bool); 27] = [
    ("a", true),
    ("Folder", true),
    ("파일.txt", true),
    ("😀", true),
    ("x.y.z", true),
    (".hidden", true),
    ("a b", true),
    ("CONSOLE", true),
    ("com10", true),
    ("NULL", true),
    ("名前", true),
    ("-", true),
    ("trailing.", false),
    ("space ", false),
    (".", false),
    ("..", false),
    ("", false),
    ("x/y", false),
    ("NUL", false),
    ("nul.txt", false),
    ("COM¹", false),
    ("LPT0", false),
    ("CONIN$", false),
    ("a:b", false),
    ("w?", false),
    ("tab\t", false),
    ("aux .log", false),
];
const LONG: &str =
    "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL";

struct Draw(u64);

impl Draw {
    fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % bound as u64) as usize
    }

    fn name(&mut self) -> (&'static str, bool) {
        let index = self.below(NAMES.len() + 4);
        NAMES.get(index).copied().unwrap_or((LONG, true))
    }
}

/// Builds an extended path from drawn components with an oracle answer and
/// checks the rule and the round trip against it.
#[test]
fn generated_paths_lose_the_prefix_only_when_the_plain_form_round_trips() {
    let mut draw = Draw(0x9e37_79b9_7f4a_7c15);
    let mut shown_count = 0;
    for _ in 0..5000 {
        let count = draw.below(6);
        let names: Vec<_> = (0..count).map(|_| draw.name()).collect();
        // An empty last component is one trailing separator.
        let names_safe = names
            .iter()
            .enumerate()
            .all(|(index, (name, safe))| *safe || (name.is_empty() && index + 1 == count));
        let tail = names
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(r"\");
        let (input, plain, root_safe) = if draw.below(3) == 0 {
            let (server, server_safe) = draw.name();
            let (share, share_safe) = draw.name();
            let root = format!(r"{server}\{share}");
            let path = if count == 0 {
                root
            } else {
                format!(r"{root}\{tail}")
            };
            let ok = server_safe && share_safe && !server.is_empty() && !share.is_empty();
            (format!(r"\\?\UNC\{path}"), format!(r"\\{path}"), ok)
        } else {
            (format!(r"\\?\C:\{tail}"), format!(r"C:\{tail}"), true)
        };
        let safe = root_safe && names_safe && plain.encode_utf16().count() < 260;
        let result = windows(&input);
        if safe {
            assert_eq!(result, plain, "{input:?}");
            assert_eq!(extended(&result), input, "{input:?}");
            shown_count += 1;
        } else {
            assert_eq!(result, input, "{input:?}");
        }
    }
    // The generator exercises both outcomes in quantity.
    assert!((500..4500).contains(&shown_count), "{shown_count}");
}
