// SPDX-License-Identifier: GPL-3.0-only

//! Removing a grouped entry from a card, with the lines that belong to it.
//!
//! Apple-style cards attach a custom label to a value by giving two lines one
//! group prefix:
//!
//! ```text
//! item1.ADR;type=HOME:;;1 Main St;Athens;;10431;GR
//! item1.X-ABLabel:Summer house
//! item1.X-ABADR:gr
//! ```
//!
//! The substrate's patcher edits a grouped value in place and never removes
//! one: dropping the entry from the model leaves the line on the card, and
//! the value is back on the next read. Taking out the value line alone would
//! be worse — the label would stay, labelling nothing. So a grouped entry is
//! removed here, on the card's text, as the group it is: the value line and
//! the annotation lines beside it go together, and every other byte of the
//! file stays as it was.
//!
//! # What counts as belonging to the value
//!
//! `X-ABLabel` and `X-ABADR`, the two annotation lines written beside a
//! grouped value. When the group holds anything else — a second value, or a
//! property this application does not know — only the entry's own line goes
//! and the rest of the group is left alone: the label still has something to
//! label, and nothing unmodelled is deleted on a guess.

use cosmic_pim_core::model::Contact;
use cosmic_pim_core::patch::logical_lines;

/// The lines that annotate a grouped value and mean nothing without it.
const ANNOTATIONS: &[&str] = &["X-ABLABEL", "X-ABADR"];

/// One grouped entry, named the way the card names it: `item1.ADR` is
/// `{ property: "ADR", group: "item1" }`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub property: &'static str,
    pub group: String,
}

/// Every grouped email, phone, website and address on a contact.
#[must_use]
pub fn entries(contact: &Contact) -> Vec<Entry> {
    let typed = [
        ("EMAIL", &contact.emails),
        ("TEL", &contact.phones),
        ("URL", &contact.urls),
    ];
    let mut out = Vec::new();
    for (property, values) in typed {
        for group in values.iter().filter_map(|value| value.group.clone()) {
            out.push(Entry { property, group });
        }
    }
    for group in contact.addresses.iter().filter_map(|a| a.group.clone()) {
        out.push(Entry {
            property: "ADR",
            group,
        });
    }
    out
}

/// `raw` without the given entries of the card carrying `uid`.
///
/// Every line that is kept is written back byte for byte, folding and line
/// terminators included, and no other card in the file is touched — the same
/// group name in the next card is that card's own.
///
/// Returns `None` when `raw` holds several cards and none of them is this
/// one, as the substrate's patchers do.
#[must_use]
pub fn remove(raw: &str, uid: &str, removed: &[Entry]) -> Option<String> {
    let target = cosmic_pim_core::vcard::vcard_index_of(raw, uid)?;
    let lines = logical_lines(raw);

    // Which lines are the target card's own properties: inside the
    // `target`-th top-level VCARD, outside anything nested in it.
    let mut own = vec![false; lines.len()];
    let (mut card, mut inside, mut nested) = (0usize, false, 0usize);
    for (index, line) in lines.iter().enumerate() {
        if let Some(component) = line.begins() {
            if inside {
                nested += 1;
            } else if component == "VCARD" {
                inside = true;
            }
        } else if line.ends().is_some() {
            if nested > 0 {
                nested -= 1;
            } else if inside {
                inside = false;
                card += 1;
            }
        } else {
            own[index] = inside && nested == 0 && card == target;
        }
    }

    let in_group = |index: usize, group: &str| {
        own[index]
            && lines[index]
                .group()
                .is_some_and(|g| g.eq_ignore_ascii_case(group))
    };

    let mut drop = vec![false; lines.len()];
    for entry in removed {
        for (index, line) in lines.iter().enumerate() {
            if in_group(index, &entry.group) && line.name() == entry.property {
                drop[index] = true;
            }
        }
    }
    // A group left holding nothing but annotations has lost what they
    // annotated, so they go too. Decided once every entry is marked: two
    // entries removed from one group must not each see the other as staying.
    for entry in removed {
        let rest: Vec<usize> = (0..lines.len())
            .filter(|&index| in_group(index, &entry.group) && !drop[index])
            .collect();
        if rest
            .iter()
            .all(|&index| ANNOTATIONS.contains(&lines[index].name().as_str()))
        {
            for index in rest {
                drop[index] = true;
            }
        }
    }

    Some(
        lines
            .iter()
            .zip(drop)
            .filter(|(_, dropped)| !dropped)
            .map(|(line, _)| line.raw())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(property: &'static str, group: &str) -> Entry {
        Entry {
            property,
            group: group.to_owned(),
        }
    }

    const APPLE: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada Lovelace\r\n\
EMAIL;type=WORK:ada@work.example\r\n\
item1.EMAIL;type=INTERNET:ada@home.example\r\nitem1.X-ABLabel:Summer house\r\n\
item2.TEL;type=pref:+30 210 1234567\r\nitem2.X-ABLabel:Boat\r\n\
item3.ADR;type=HOME:;;1 Main St;Athens;;10431;GR\r\nitem3.X-ABLabel:Winter\r\n\
item3.X-ABADR:gr\r\n\
item4.URL:https://ada.example\r\nitem4.X-ABLabel:_$!<HomePage>!$_\r\n\
PHOTO;ENCODING=b:AAAABBBB\r\nX-ABShowAs:COMPANY\r\nEND:VCARD\r\n";

    /// The defect this module exists for: each of the four kinds goes with
    /// its label, and nothing else on the card moves.
    #[test]
    fn a_grouped_entry_goes_with_its_label_whatever_kind_it_is() {
        for (property, group, gone) in [
            (
                "EMAIL",
                "item1",
                "item1.EMAIL;type=INTERNET:ada@home.example\r\nitem1.X-ABLabel:Summer house\r\n",
            ),
            (
                "TEL",
                "item2",
                "item2.TEL;type=pref:+30 210 1234567\r\nitem2.X-ABLabel:Boat\r\n",
            ),
            (
                "ADR",
                "item3",
                "item3.ADR;type=HOME:;;1 Main St;Athens;;10431;GR\r\nitem3.X-ABLabel:Winter\r\n\
item3.X-ABADR:gr\r\n",
            ),
            (
                "URL",
                "item4",
                "item4.URL:https://ada.example\r\nitem4.X-ABLabel:_$!<HomePage>!$_\r\n",
            ),
        ] {
            let out = remove(APPLE, "ada", &[entry(property, group)]).unwrap();
            assert_eq!(
                out,
                APPLE.replace(gone, ""),
                "removing {group}.{property} changed something else, or left its label behind"
            );
            assert!(
                !out.contains(&format!("{group}.")),
                "{group} still has a line on the card:\n{out}"
            );
        }
    }

    #[test]
    fn several_entries_go_in_one_pass() {
        let out = remove(
            APPLE,
            "ada",
            &[entry("EMAIL", "item1"), entry("ADR", "item3")],
        )
        .unwrap();
        assert!(!out.contains("item1."), "{out}");
        assert!(!out.contains("item3."), "{out}");
        assert!(out.contains("item2.TEL"), "{out}");
        assert!(out.contains("item4.URL"), "{out}");
    }

    /// One label over two values: taking one value out leaves the label with
    /// the other, which it still labels.
    #[test]
    fn a_group_shared_with_another_value_keeps_its_label() {
        let card = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada\r\n\
item1.EMAIL:ada@boat.example\r\nitem1.TEL:+30 210 1234567\r\nitem1.X-ABLabel:Boat\r\n\
END:VCARD\r\n";
        let out = remove(card, "ada", &[entry("EMAIL", "item1")]).unwrap();
        assert_eq!(out, card.replace("item1.EMAIL:ada@boat.example\r\n", ""));

        // Both values removed: now the label labels nothing, and goes.
        let out = remove(
            card,
            "ada",
            &[entry("EMAIL", "item1"), entry("TEL", "item1")],
        )
        .unwrap();
        assert!(!out.contains("item1."), "{out}");
    }

    /// A property this application does not know is not deleted on a guess.
    #[test]
    fn an_unknown_line_in_the_group_is_left_alone() {
        let card = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada\r\n\
item1.ADR:;;1 Main St;Athens;;;GR\r\nitem1.X-ABLabel:Home\r\nitem1.X-VENDOR-PIN:42\r\n\
END:VCARD\r\n";
        let out = remove(card, "ada", &[entry("ADR", "item1")]).unwrap();
        assert_eq!(
            out,
            card.replace("item1.ADR:;;1 Main St;Athens;;;GR\r\n", "")
        );
    }

    /// Every export is one file of many cards, and every one of them starts
    /// its groups at `item1`.
    #[test]
    fn the_same_group_name_in_another_card_is_that_cards_own() {
        let two = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada\r\n\
item1.EMAIL:ada@home.example\r\nitem1.X-ABLabel:Home\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nUID:bob\r\nFN:Bob\r\n\
item1.EMAIL:bob@home.example\r\nitem1.X-ABLabel:Home\r\nEND:VCARD\r\n";

        let out = remove(two, "bob", &[entry("EMAIL", "item1")]).unwrap();
        assert!(out.contains("item1.EMAIL:ada@home.example\r\nitem1.X-ABLabel:Home\r\n"));
        assert!(!out.contains("bob@home.example"), "{out}");
        assert_eq!(out.matches("X-ABLabel").count(), 1, "{out}");

        assert!(
            remove(two, "nobody", &[entry("EMAIL", "item1")]).is_none(),
            "a card that is not in the file was treated as one that is"
        );
    }

    /// `item1` is not `item10`, and a label folded over two lines is one
    /// line to remove.
    #[test]
    fn group_names_match_whole_and_folded_lines_go_whole() {
        let card = "BEGIN:VCARD\nVERSION:3.0\nUID:ada\nFN:Ada\n\
item1.EMAIL:ada@home.example\nitem1.X-ABLabel:A label long enough that the \n server folded it\n\
item10.EMAIL:ada@other.example\nitem10.X-ABLabel:Other\nEND:VCARD\n";
        let out = remove(card, "ada", &[entry("EMAIL", "item1")]).unwrap();
        assert_eq!(
            out,
            "BEGIN:VCARD\nVERSION:3.0\nUID:ada\nFN:Ada\n\
item10.EMAIL:ada@other.example\nitem10.X-ABLabel:Other\nEND:VCARD\n"
        );
    }

    #[test]
    fn a_card_with_nothing_to_remove_is_returned_unchanged() {
        assert_eq!(remove(APPLE, "ada", &[]).unwrap(), APPLE);
        assert_eq!(
            remove(APPLE, "ada", &[entry("EMAIL", "item9")]).unwrap(),
            APPLE
        );
    }

    #[test]
    fn the_grouped_entries_of_a_contact_are_read_off_all_four_lists() {
        let contact = cosmic_pim_core::vcard::parse_vcards(APPLE, "personal", "ada.vcf").remove(0);
        assert_eq!(
            entries(&contact),
            [
                entry("EMAIL", "item1"),
                entry("TEL", "item2"),
                entry("URL", "item4"),
                entry("ADR", "item3"),
            ]
        );
    }
}
