//! English: the window's canonical strings, each key named after its text.
//!
//! Only what the window alone says lives here. Errors and diagnostics are the
//! library's (`raven::i18n`), and the Preferences skeleton, themes and accents
//! are colony-ui's; a test refuses a key that repeats one of theirs. A key here
//! must be in `fr.rs` too.

#[rustfmt::skip]
pub(super) const STRINGS: &[(&str, &str)] = &[
    // The sidebar.
    ("nav_environments", "Environments"),
    ("nav_bases", "Bases"),
    ("nav_diagnostics", "Diagnostics"),
    // Buttons that act on an environment.
    ("action_start", "Start"),
    ("action_stop", "Stop"),
    ("action_remove", "Remove"),
    ("action_install", "Install…"),
    ("action_detach", "Detach"),
    ("action_reproject", "Reproject"),
    ("action_deploy", "Deploy"),
    ("action_stop_session", "Stop the session"),
    ("action_stop_holders", "Stop everything using it"),
    // The environments screen and an environment's card.
    ("envs_empty_title", "No environments yet"),
    ("envs_empty_hint", "Create one with:  raven env create <name> --base <base>"),
    ("card_not_running", "not running"),
    ("card_running_one", "running - {n} process"),
    ("card_running_other", "running - {n} processes"),
    // An environment's detail.
    ("detail_back", "← Environments"),
    ("detail_session", "Session"),
    ("detail_direct3d", "Direct3D"),
    ("detail_runtime_missing", "{runtime}: not installed"),
    ("detail_separate_runtimes", "They are separate runtimes, not versions of one."),
    ("detail_devices", "Devices"),
    ("detail_no_devices", "None attached. Attach one with:  raven env attach <name> /dev/sdX"),
    ("detail_registry", "Registry"),
    ("detail_registry_hint", "Re-run the projection after editing registry-rules.toml."),
    ("install_d3d_notice", "Install it from a build you already have:  raven env {which} {env} --from <path>"),
    // The bases screen and its deployment.
    ("bases_empty", "No bases yet"),
    ("bases_environments_one", "{n} environment"),
    ("bases_environments_other", "{n} environments"),
    ("deploy_title", "Deploy a base"),
    ("deploy_image", "Path to install.wim"),
    ("deploy_edition", "Edition index"),
    ("deploy_name", "Name"),
    ("deploy_progress", "{what} - {percent}%"),
    ("deploy_starting", "Starting"),
    ("deploy_phase_creating_files", "Creating files"),
    ("deploy_phase_extracting", "Extracting file data"),
    ("deploy_incomplete_form", "Fill in the image path, a numeric edition, and a name."),
    ("deploy_failed", "Deployment failed. {reason}"),
    ("deploy_would_not_start", "`raven base deploy` would not start: {why}"),
    ("deploy_no_reason", "It gave no reason before it stopped."),
    // The diagnostics screen.
    ("diagnostics_empty", "No diagnostics yet"),
    // A failure the window can do something about.
    ("offer_session_holds", "A session is holding {name}'s C:, which is what makes launches fast."),
    ("offer_busy", "{name} is still in use by {holders}. Stopping it ends them."),
    // What raven-gui says on a command line instead of opening a window.
    ("answer_help", "raven-gui - Raven's administration window. It takes no arguments.\nThe command line is `raven`; run `raven --help` for it."),
    ("answer_anchor_needs_name", "session-anchor needs an environment name"),
    ("answer_unknown", "raven-gui does not answer {quoted}; run `raven {verb}` instead."),
];
