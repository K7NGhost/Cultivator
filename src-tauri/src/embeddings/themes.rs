//! Investigative themes. A question about behaviour ("people being
//! discreet", "threats") names an idea the messages themselves never state:
//! on a 100,000-message phone, "New burner number soon." ranked around
//! 31,000th for "discreet". Each theme lists example messages, searched
//! alongside the question so the vector search finds the concrete wording,
//! and a description the precision check judges matches against. Concrete
//! topics ("money", "invoices", "meeting") already work without a theme.

pub(super) struct Theme {
    pub name: &'static str,
    /// Question words (lowercase) that call up the theme.
    triggers: &'static [&'static str],
    pub examples: &'static [&'static str],
    pub description: &'static str,
}

static THEMES: &[Theme] = &[
    Theme {
        name: "secrecy",
        triggers: &[
            "avoid", "avoiding", "caught", "concealing", "covering", "covert", "detection",
            "discreet", "discrete", "discretion", "evade", "evading", "hid", "hidden", "hide",
            "hiding", "lowkey", "secret", "secretive", "secrecy", "secrets", "shady", "sneaky",
            "suspicious", "tracks", "untraceable",
        ],
        examples: &[
            "Delete these texts.",
            "Delete this after you read it.",
            "Use the burner phone from now on.",
            "New number, stop using this one.",
            "Don't put this in writing.",
            "Call me, don't text.",
            "Don't use email for this.",
            "Use Signal, texts can be read.",
            "Wipe your phone.",
            "No names over text.",
            "Leave it in the usual spot.",
            "We shouldn't talk for a while.",
            "Keep this between us.",
            "If anyone asks, you don't know me.",
        ],
        description: "someone deliberately hiding their activity: telling others to delete messages or wipe phones, switching to burner phones or encrypted apps, refusing to put things in writing, using no names, agreeing on a cover story, or cutting contact to avoid attention",
    },
    Theme {
        name: "destroying evidence",
        triggers: &[
            "delete", "deleted", "deleting", "destroy", "destroyed", "destroying", "erase",
            "erased", "erasing", "evidence", "shred", "shredded", "shredding", "wipe", "wiped",
            "wiping",
        ],
        examples: &[
            "Delete these texts.",
            "Wipe your phone.",
            "Get rid of the paperwork.",
            "Shred the documents.",
            "Clean up the records before the audit.",
            "Erase the files.",
        ],
        description: "destroying or hiding evidence: deleting messages or files, wiping phones, shredding or cleaning up records",
    },
    Theme {
        name: "threats",
        triggers: &[
            "afraid", "danger", "dangerous", "gun", "guns", "harm", "hurt", "intimidate",
            "intimidating", "intimidation", "kill", "knife", "revenge", "scared", "threat",
            "threaten", "threatened", "threatening", "threats", "violence", "violent", "weapon",
            "weapons",
        ],
        examples: &[
            "You'll regret this.",
            "I know where you live.",
            "Keep your mouth shut or else.",
            "Watch your back.",
            "I'll take care of him.",
            "Bring the gun.",
        ],
        description: "threats or violence: intimidating someone, warning them to stay quiet, or talking about hurting someone or weapons",
    },
    Theme {
        name: "fraud",
        triggers: &[
            "cooked", "cooking", "doctored", "embezzle", "embezzlement", "embezzling", "fake",
            "faked", "falsified", "falsify", "falsifying", "forged", "forgery", "fraud",
            "fraudulent", "scheme",
        ],
        examples: &[
            "Invoice it as repairs.",
            "Change the numbers before they check.",
            "Backdate the contract.",
            "Make the books look clean.",
            "Redo the invoices.",
            "Put it down as a business expense.",
        ],
        description: "falsifying records: fake invoices, changed numbers, cooked books, backdated documents",
    },
    Theme {
        name: "drugs",
        triggers: &[
            "cocaine", "coke", "dealer", "dealers", "dealing", "drug", "drugs", "fentanyl",
            "heroin", "marijuana", "meth", "narcotics", "pills", "plug", "weed",
        ],
        examples: &[
            "Got the pack ready.",
            "How many grams?",
            "Need to re-up.",
            "Can you get me some pills?",
            "The plug is out till Friday.",
        ],
        description: "drugs: buying, selling or using drugs, including slang",
    },
];

/// The question's words that call up none of `themes` ("money" in "hiding
/// money"), joined back into a phrase.
pub(super) fn other_words(themes: &[&Theme], words: &[String]) -> String {
    words
        .iter()
        .filter(|word| !themes.iter().any(|theme| theme.triggers.contains(&word.as_str())))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The themes any of the question's words call up.
pub(super) fn matching<'a>(words: impl IntoIterator<Item = &'a str>) -> Vec<&'static Theme> {
    let words = words.into_iter().collect::<Vec<_>>();
    THEMES
        .iter()
        .filter(|theme| words.iter().any(|word| theme.triggers.contains(word)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behaviour_words_call_up_their_themes() {
        let names = |words: &[&str]| {
            matching(words.iter().copied())
                .iter()
                .map(|theme| theme.name)
                .collect::<Vec<_>>()
        };

        assert_eq!(names(&["discreet"]), vec!["secrecy"]);
        assert_eq!(names(&["deleting", "evidence"]), vec!["destroying evidence"]);
        assert_eq!(names(&["hiding", "evidence"]), vec!["secrecy", "destroying evidence"]);
        assert!(names(&["money", "meeting", "invoices"]).is_empty());
    }

    #[test]
    fn keeps_the_topic_of_a_mixed_question() {
        let themes = matching(["hiding", "money"]);
        let words = ["hiding".to_string(), "money".to_string()];

        assert_eq!(other_words(&themes, &words), "money");
        assert_eq!(other_words(&themes, &words[..1]), "");
    }
}
