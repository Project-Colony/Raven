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

/// A message to show, and optionally a button to show beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub message: String,
    pub action: Option<Action>,
}

pub fn explain(error: &Error) -> Offer {
    match error {
        Error::SessionHolds(name) => Offer {
            message: format!(
                "A session is holding {name}'s C:, which is what makes launches fast."
            ),
            action: Some(Action::Stop(name.clone())),
        },
        Error::EnvironmentBusy { name, holders } => Offer {
            message: format!("{name} is still in use by {holders}."),
            action: Some(Action::Stop(name.clone())),
        },
        other => Offer {
            message: other.to_string(),
            action: None,
        },
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
    fn an_error_with_no_obvious_action_keeps_its_own_words() {
        let e = Error::NoWine;
        let offer = explain(&e);
        assert_eq!(offer.action, None);
        assert_eq!(offer.message, e.to_string());
    }
}
