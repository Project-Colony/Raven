//! Turning a library error into something a window can offer.
//!
//! Raven's messages are written for a terminal and several carry a command to
//! run. Telling someone using a window to open a terminal is an admission of
//! failure, so the errors with an obvious action become that action. The rest
//! keep their own words: the text is good, and a GUI-flavoured paraphrase
//! would lose the part that explains *why*.

use raven::Error;

/// What the window offers to do about an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Stop(String),
}

/// What raised the banner, which decides both how it is painted and what is
/// allowed to take it away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Reading the machine's state failed. The next successful read means the
    /// condition is gone, so that read clears it.
    LoadError,
    /// An action failed. A read must never clear this: the environments screen
    /// reloads every two seconds, and an error that survives for one poll
    /// interval is an error nobody gets to act on.
    ActionError,
    /// Guidance, not a failure - what to type for something the window cannot
    /// do yet. Painted neutrally, because `error` ink on `error_bg` would tell
    /// the user something went wrong when nothing did.
    Notice,
}

/// A message to show, and optionally a button to show beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub kind: Kind,
    pub message: String,
    pub action: Option<Action>,
}

impl Offer {
    pub fn load_error(message: String) -> Self {
        Self {
            kind: Kind::LoadError,
            message,
            action: None,
        }
    }

    pub fn action_error(message: String) -> Self {
        Self {
            kind: Kind::ActionError,
            message,
            action: None,
        }
    }

    pub fn notice(message: String) -> Self {
        Self {
            kind: Kind::Notice,
            message,
            action: None,
        }
    }
}

pub fn explain(error: &Error) -> Offer {
    match error {
        Error::SessionHolds(name) => Offer {
            kind: Kind::ActionError,
            message: format!(
                "A session is holding {name}'s C:, which is what makes launches fast."
            ),
            action: Some(Action::Stop(name.clone())),
        },
        Error::EnvironmentBusy { name, holders } => Offer {
            kind: Kind::ActionError,
            message: format!("{name} is still in use by {holders}."),
            action: Some(Action::Stop(name.clone())),
        },
        other => Offer::action_error(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raven::Error;

    #[test]
    fn a_session_holding_an_environment_offers_to_stop_it() {
        let offer = explain(&Error::SessionHolds("games".into()));
        assert_eq!(offer.action, Some(Action::Stop("games".into())));
        assert!(
            !offer.message.contains("raven env stop"),
            "the button replaces the command; repeating it is noise: {}",
            offer.message
        );
    }

    #[test]
    fn what_an_action_raises_is_marked_as_such_so_a_refresh_cannot_erase_it() {
        // The environments screen reloads every two seconds. If these came back
        // as `LoadError`, the next poll would take the message and its button
        // away before anyone had read either.
        assert_eq!(
            explain(&Error::SessionHolds("games".into())).kind,
            Kind::ActionError
        );
        assert_eq!(explain(&Error::NoWine).kind, Kind::ActionError);
    }

    #[test]
    fn an_error_with_no_obvious_action_keeps_its_own_words() {
        let e = Error::NoWine;
        let offer = explain(&e);
        assert_eq!(offer.action, None);
        assert_eq!(offer.message, e.to_string());
    }
}
