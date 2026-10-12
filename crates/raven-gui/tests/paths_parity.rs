//! `raven::paths` gives the same directories as `colony_ui::paths`.
//!
//! The library works its directories out itself, so the command line does not
//! link a GUI toolkit to compute a directory name. That makes it a second
//! copy of the org's canonical answer, and this pins the copy to it: with the
//! XDG variables set, and with only HOME to go on.

use std::path::Path;

use colony_ui::paths::locate;

const XDG: [(&str, &str); 3] = [
    ("XDG_CONFIG_HOME", "config"),
    ("XDG_DATA_HOME", "data"),
    ("XDG_CACHE_HOME", "cache"),
];

fn same_three() {
    assert_eq!(
        raven::paths::config_dir().unwrap(),
        locate::config_dir("Raven").unwrap()
    );
    assert_eq!(
        raven::paths::data_dir().unwrap(),
        locate::data_dir("Raven").unwrap()
    );
    assert_eq!(
        raven::paths::cache_dir().unwrap(),
        locate::cache_dir("Raven").unwrap()
    );
}

#[test]
fn ravens_directories_are_colony_uis() {
    let root = std::env::temp_dir().join(format!("raven-paths-{}", std::process::id()));
    let set = |key: &str, value: &Path| {
        // SAFETY: this is the only test in its binary, so no other thread
        // reads the environment while it changes.
        unsafe { std::env::set_var(key, value) }
    };

    set("HOME", &root.join("home"));
    for (key, dir) in XDG {
        set(key, &root.join(dir));
    }
    same_three();

    for (key, _) in XDG {
        // SAFETY: as above.
        unsafe { std::env::remove_var(key) }
    }
    same_three();
}
