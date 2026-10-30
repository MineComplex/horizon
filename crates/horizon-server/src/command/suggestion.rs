//! Completion suggestions: `Suggestion`, `IntegerSuggestion`, `Suggestions` and `SuggestionsBuilder`.

use std::{cmp::Ordering, collections::HashMap};

use super::StringRange;
use crate::azalea::chat::FormattedText;

/// One completion: replace `range` of the input with `text`.
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub range: StringRange,
    pub text: String,
    pub tooltip: Option<FormattedText>,
    /// Set for an `IntegerSuggestion`, which sorts numerically against other integer suggestions.
    pub int_value: Option<i32>,
}

impl Suggestion {
    pub fn new(range: StringRange, text: impl Into<String>) -> Self {
        Self {
            range,
            text: text.into(),
            tooltip: None,
            int_value: None,
        }
    }

    /// `apply`: the input with this suggestion applied.
    pub fn apply(&self, input: &str) -> String {
        format!(
            "{}{}{}",
            &input[..self.range.start],
            self.text,
            &input[self.range.end..]
        )
    }

    /// `expand`: the same suggestion over the wider `range`, padded with the input around it. An
    /// expanded integer suggestion becomes a plain one, as in Brigadier.
    fn expand(self, command: &str, range: StringRange) -> Self {
        if range == self.range {
            return self;
        }
        let text = format!(
            "{}{}{}",
            &command[range.start..self.range.start],
            self.text,
            &command[self.range.end..range.end]
        );
        Self {
            range,
            text,
            tooltip: self.tooltip,
            int_value: None,
        }
    }
}

/// `Suggestion.compareToIgnoreCase`; `IntegerSuggestion` compares numerically with another one.
fn compare_ignore_case(a: &Suggestion, b: &Suggestion) -> Ordering {
    match (a.int_value, b.int_value) {
        (Some(x), Some(y)) => x.cmp(&y),
        _ => compare_strings_ignore_case(&a.text, &b.text),
    }
}

/// `String.compareToIgnoreCase`: compare character by character after upper- then lower-casing.
pub fn compare_strings_ignore_case(a: &str, b: &str) -> Ordering {
    let fold = |c: char| {
        let upper = single_case(c.to_uppercase()).unwrap_or(c);
        single_case(upper.to_lowercase()).unwrap_or(upper)
    };
    let mut x = a.chars();
    let mut y = b.chars();
    loop {
        match (x.next(), y.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c1), Some(c2)) if c1 != c2 => {
                let (l1, l2) = (fold(c1), fold(c2));
                if l1 != l2 {
                    return l1.cmp(&l2);
                }
            }
            _ => {}
        }
    }
}

/// Java's `Character.toUpperCase(char)` maps a character to one character; a multi-character mapping
/// (`ß` → `SS`) leaves it unchanged.
fn single_case(mut mapped: impl Iterator<Item = char>) -> Option<char> {
    let first = mapped.next()?;
    mapped.next().is_none().then_some(first)
}

/// `Suggestions`: completions over a common range, sorted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Suggestions {
    pub range: StringRange,
    pub list: Vec<Suggestion>,
}

impl Suggestions {
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// `Suggestions.create`: expand every suggestion to the range they cover together, drop duplicates
    /// and sort case-insensitively.
    ///
    /// Brigadier collects into a `HashSet` before its (stable) sort, so suggestions that compare equal
    /// come out in hash order there; here they keep the order they were made in.
    pub fn create(command: &str, suggestions: Vec<Suggestion>) -> Self {
        let (Some(start), Some(end)) = (
            suggestions.iter().map(|s| s.range.start).min(),
            suggestions.iter().map(|s| s.range.end).max(),
        ) else {
            return Self::default();
        };
        let range = StringRange { start, end };
        let mut list: Vec<Suggestion> = Vec::with_capacity(suggestions.len());
        // `Suggestion.equals`: range (now common), text, tooltip, and the integer value.
        let mut seen: HashMap<String, Vec<usize>> = HashMap::with_capacity(suggestions.len());
        for suggestion in suggestions {
            let suggestion = suggestion.expand(command, range);
            let same_text = seen.entry(suggestion.text.clone()).or_default();
            if same_text.iter().any(|&i| {
                list[i].tooltip == suggestion.tooltip && list[i].int_value == suggestion.int_value
            }) {
                continue;
            }
            same_text.push(list.len());
            list.push(suggestion);
        }
        sort_stable(&mut list);
        Self { range, list }
    }

    /// `Suggestions.merge`: one child's suggestions as they are; several children's joined and sorted.
    pub fn merge(command: &str, mut input: Vec<Suggestions>) -> Self {
        match input.len() {
            0 => Self::default(),
            1 => input.pop().unwrap_or_default(),
            _ => Self::create(command, input.into_iter().flat_map(|s| s.list).collect()),
        }
    }
}

/// A stable binary insertion sort. `compareToIgnoreCase` is not a total order when integer and text
/// suggestions mix, and `slice::sort_by` may panic on such a comparator; this never does.
fn sort_stable(list: &mut Vec<Suggestion>) {
    let mut sorted: Vec<Suggestion> = Vec::with_capacity(list.len());
    for suggestion in list.drain(..) {
        // Java's binarySort compares the new element against the sorted ones, in that order.
        let at = sorted.partition_point(|s| compare_ignore_case(&suggestion, s) != Ordering::Less);
        sorted.insert(at, suggestion);
    }
    *list = sorted;
}

/// `SuggestionsBuilder`: suggestions for the text from `start` to the end of `input` (the input up to
/// the cursor).
#[derive(Debug)]
pub struct SuggestionsBuilder<'a> {
    input: &'a str,
    start: usize,
    remaining_lowercase: String,
    result: Vec<Suggestion>,
}

impl<'a> SuggestionsBuilder<'a> {
    pub fn new(input: &'a str, start: usize) -> Self {
        Self {
            input,
            start,
            remaining_lowercase: input[start..].to_lowercase(),
            result: Vec::new(),
        }
    }

    pub fn input(&self) -> &'a str {
        self.input
    }

    pub fn start(&self) -> usize {
        self.start
    }

    pub fn remaining(&self) -> &'a str {
        &self.input[self.start..]
    }

    /// `getRemainingLowerCase` (`toLowerCase(Locale.ROOT)`).
    pub fn remaining_lowercase(&self) -> &str {
        &self.remaining_lowercase
    }

    fn range(&self) -> StringRange {
        StringRange {
            start: self.start,
            end: self.input.len(),
        }
    }

    /// `suggest(text)`: skipped when it is exactly what was typed.
    pub fn suggest(&mut self, text: impl Into<String>) -> &mut Self {
        let text = text.into();
        if text != self.remaining() {
            self.result.push(Suggestion::new(self.range(), text));
        }
        self
    }

    pub fn suggest_with_tooltip(
        &mut self,
        text: impl Into<String>,
        tooltip: FormattedText,
    ) -> &mut Self {
        let text = text.into();
        if text != self.remaining() {
            let mut suggestion = Suggestion::new(self.range(), text);
            suggestion.tooltip = Some(tooltip);
            self.result.push(suggestion);
        }
        self
    }

    /// `suggest(int)`: an `IntegerSuggestion` (added even when it equals the typed text, as in Brigadier).
    pub fn suggest_int(&mut self, value: i32) -> &mut Self {
        let mut suggestion = Suggestion::new(self.range(), value.to_string());
        suggestion.int_value = Some(value);
        self.result.push(suggestion);
        self
    }

    /// `createOffset`: a new builder over the same input from `start`.
    pub fn create_offset(&self, start: usize) -> SuggestionsBuilder<'a> {
        SuggestionsBuilder::new(self.input, start)
    }

    /// `add`: take another builder's suggestions.
    pub fn add(&mut self, other: SuggestionsBuilder) -> &mut Self {
        self.result.extend(other.result);
        self
    }

    /// `build`.
    pub fn build(self) -> Suggestions {
        Suggestions::create(self.input, self.result)
    }
}
