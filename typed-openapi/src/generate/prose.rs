//! What becomes of a vendor's prose on its way into a generated doc comment.
//!
//! Every description in the document is a doc comment in `types.rs` and
//! `ops.rs`, so this is the one place that decides what a vendor's sentence can
//! do to the crate that compiles it.

use syn::visit_mut::VisitMut;

/// A vendor's description, kept where rustdoc will not run it.
///
/// Every description in the document becomes a doc comment, and rustdoc
/// compiles and executes the code blocks in a doc comment. Markdown makes a
/// code block out of three ordinary shapes of prose: a run of lines indented
/// four spaces or more, a list item whose content sits five columns from its
/// marker, and a fence that names no language. A vendor writing a nested list,
/// a column of items lined up under the widest marker, or a hanging example
/// writes all three without meaning any of them, and what the adopter gets is
/// `cargo test --doc` failing on the vendor's sentences — which no lint
/// allowance reaches, because a doctest is executed rather than linted.
///
/// So each line is made unrunnable where it would otherwise be run, and left
/// alone everywhere else. The generator is what put the prose where rustdoc
/// would execute it, so the generator is where it is made safe; an adopter
/// switching doctests off for the whole crate would be hiding this and taking
/// their own hand-written files with it.
///
/// # What may be edited, and what may not
///
/// **The generated page has to render what the vendor wrote.** Every edit here
/// is chosen against that: it changes bytes the vendor has no stake in, so that
/// the rendering is the one they meant. Capping an indentation keeps a nested
/// list nested instead of letting it become a code block; pulling a hanging
/// item's content back keeps it an item; writing a bullet as `-` keeps the
/// bullet, which rustc would otherwise eat. What none of them may do is change
/// what the page says — the inside of a fence is the vendor's sample and is
/// passed through untouched, and a fence's language is renamed rather than
/// dropped, because the word is what a reader's highlighting goes by.
pub(super) struct Prose;

impl VisitMut for Prose {
    fn visit_attribute_mut(&mut self, attr: &mut syn::Attribute) {
        let syn::Meta::NameValue(pair) = &mut attr.meta else {
            return;
        };
        if !pair.path.is_ident("doc") {
            return;
        }
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(text),
            ..
        }) = &mut pair.value
        else {
            return;
        };
        *text = syn::LitStr::new(&unrunnable(&text.value()), text.span());
    }
}

/// How wide Markdown counts a tab when it measures indentation.
const TAB: usize = 4;

/// The deepest indentation that cannot open a code block.
///
/// Four opens one, so three is what is left. It is enough to keep a nested list
/// nested — a list marker needs only to reach its parent's content column — so
/// what capping costs is the depth of an unusually deep one, and what it buys
/// is a crate whose tests run.
const KEEP: usize = 3;

/// The widest gap between a list marker and its content that cannot open a
/// code block.
///
/// Markdown measures a list item's content from the end of its marker rather
/// than from the start of the line, so this is one wider than [`KEEP`]: five
/// columns after a marker open a code block inside the item, four do not.
/// Measured on `-`, `+`, `*` and an ordered marker alike.
///
/// Four is safe only where the marker survives to hold the gap off the margin,
/// which is [`dashed`]'s doing; [`unhung`] says what happens where it does
/// not.
const HANG: usize = 4;

/// The bullet a list marker is written with.
///
/// `-` rather than `*`, and [`dashed`] says why.
const BULLET: char = '-';

/// The most digits Markdown reads as one ordered list marker.
const ORDERED: usize = 9;

/// `prose` with nothing in it that rustdoc would compile.
fn unrunnable(prose: &str) -> String {
    let capped = capped_lines(prose);
    if !capped.contains('\n') {
        // One line is one `///`, where nothing is indented and there is
        // nothing to strip.
        return capped;
    }
    // More than one line is a `/* */` comment, and `rustfmt` indents its body
    // to the item it sits on — every line but the first, which stays flush
    // against the opening `/*`. rustc strips the indentation a comment's lines
    // share, and a flush first line holds that strip down to almost nothing, so
    // the rest of the body arrives still carrying the column the item sits at:
    // a paragraph the vendor indented three spaces lands six in, which is a
    // code block. Opening on a blank line puts the first line inside the
    // indented body with the others, where the strip reaches it, and that same
    // paragraph lands back on its three.
    format!("\n{capped}\n")
}

/// `prose` with no line indented deeply enough to open a code block, no list
/// item hanging its content far enough to open one, no bullet rustc will eat,
/// and no fence rustdoc would read as Rust.
///
/// A line is capped, then unhung, then dashed, then read as a fence, and the
/// first step's place in that order is the rule: Markdown reads a fence at
/// three columns or fewer, so an indented one is not a fence at all but the
/// start of an indented code block — capping it first is what turns it into the
/// fence the vendor meant, rather than leaving a block whose first line happens
/// to be three backticks. The two list steps cannot disturb that, because a run
/// of backticks is not a list marker.
fn capped_lines(prose: &str) -> String {
    let mut fenced = false;
    let lines: Vec<String> = prose
        .split('\n')
        .map(|line| {
            // Inside a fence the content is the vendor's sample, kept as they
            // wrote it — the fence above it already says nobody will run it.
            if fenced && fence(line).is_none() {
                return line.to_owned();
            }
            let tamed = dashed(&unhung(&capped(line)));
            let Some((marker, language)) = fence(&tamed) else {
                return tamed;
            };
            if fenced {
                fenced = false;
                return tamed;
            }
            fenced = true;
            if compiled(language) {
                let indent: String = tamed.chars().take_while(|c| c.is_whitespace()).collect();
                format!("{indent}{marker}{INERT}")
            } else {
                tamed
            }
        })
        .collect();
    lines.join("\n")
}

/// The language named on a fence rustdoc will not compile.
///
/// Any word it does not recognise does, and this one says what the block is.
const INERT: &str = "text";

/// The words rustdoc reads above a code block as *attributes of Rust* rather
/// than as the name of a language.
///
/// A fence carrying one of them — or carrying nothing — is a block rustdoc
/// compiles and runs, so a vendor who wrote `rust` over a line of pseudocode
/// has written a doctest without meaning to. Every other word names a language
/// rustdoc leaves alone, and the vendor's own is worth keeping: it is what a
/// reader's syntax highlighting goes by.
const COMPILED: [&str; 7] = [
    "compile_fail",
    "ignore",
    "no_run",
    "rust",
    "should_panic",
    "standalone_crate",
    "test_harness",
];

/// Would rustdoc compile a block a fence naming `language` opens?
fn compiled(language: &str) -> bool {
    let word = language.split([',', ' ', '\t']).next().unwrap_or(language);
    word.is_empty() || word.starts_with("edition") || COMPILED.contains(&word)
}

/// The marker this line fences with and the language it names, if it is a
/// fence.
///
/// A fence is three or more backticks or tildes. Whether it is indented too
/// far to be one is settled before this is asked, by capping the line.
fn fence(line: &str) -> Option<(&str, &str)> {
    let body = line.trim_start();
    let mark = ['`', '~']
        .into_iter()
        .find(|mark| body.chars().take(3).filter(|c| c == mark).count() == 3)?;
    let run = body.len() - body.trim_start_matches(mark).len();
    let (marker, language) = body.split_at(run);
    Some((marker, language.trim()))
}

/// `line` with its indentation capped at [`KEEP`].
fn capped(line: &str) -> String {
    let content = line.trim_start();
    if content.is_empty() || columns(line) <= KEEP {
        return line.to_owned();
    }
    format!("{}{content}", " ".repeat(KEEP))
}

/// How many columns the whitespace at the start of `text` is worth.
///
/// Markdown counts a tab as [`TAB`], and both caps are stated in columns, so
/// this is the one place that reading is made.
fn columns(text: &str) -> usize {
    text.chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { TAB } else { 1 })
        .sum()
}

/// `line` with the gap between a list marker and its content capped at
/// [`HANG`].
///
/// This is the code block [`capped`] cannot see. A vendor lining the text of
/// several items up under the widest marker writes five spaces after a short
/// one, and Markdown reads content that far from a marker as an indented code
/// block *inside* the item — while the line's own indentation, which is all
/// [`capped`] measures, is nothing at all.
///
/// The cap holds at four only because [`dashed`] has taken `*` off the front of
/// every bullet. Where a `*` survives, rustc eats it out of a `/* */` comment
/// and leaves the gap standing alone as ordinary indentation, and four columns
/// of that under a blank line opens a block of its own. The two rules are one
/// rule, and `PROSE`'s list set off by a blank line is what holds them
/// together.
///
/// The line is left alone when the marker carries no content: a marker on its
/// own opens nothing, and there is no gap to measure.
fn unhung(line: &str) -> String {
    let content = line.trim_start();
    let Some(width) = list_marker(content) else {
        return line.to_owned();
    };
    let (indent, rest) = line.split_at(line.len() - content.len());
    let (marker, after) = rest.split_at(width);
    let text = after.trim_start();
    if text.is_empty() || columns(after) <= HANG {
        return line.to_owned();
    }
    format!("{indent}{marker}{}{text}", " ".repeat(HANG))
}

/// `line` with a `*` bullet written as [`BULLET`].
///
/// rustc rebuilds a `/* */` comment by stripping a leading `*` off every line
/// whenever all of the lines it weighs carry one at the same column, which is
/// the shape a vendor's bullet list takes exactly. What reaches the reader is
/// then a run of sentences where the vendor wrote a list, and nothing on the
/// page says a marker went missing.
///
/// Markdown draws the same bullet for `-` and `*`, so this is one character the
/// vendor has no stake in and the rendering is the one they wrote. `+` and the
/// ordered markers are left alone: rustc eats none of them.
fn dashed(line: &str) -> String {
    let content = line.trim_start();
    if !content.starts_with('*') || list_marker(content).is_none() {
        return line.to_owned();
    }
    let (indent, rest) = line.split_at(line.len() - content.len());
    let (_, after) = rest.split_at(1);
    format!("{indent}{BULLET}{after}")
}

/// How wide the list marker at the start of `content` is, if it is one.
///
/// Markdown's markers are `-`, `+` or `*`, and up to [`ORDERED`] digits
/// followed by `.` or `)`. Whitespace after it is what makes it a marker at
/// all, which is also what keeps `*emphasis*` at the start of a line out of
/// this.
fn list_marker(content: &str) -> Option<usize> {
    let width = if content.starts_with(['-', '+', '*']) {
        1
    } else {
        let digits = content.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 || digits > ORDERED {
            return None;
        }
        if !content.get(digits..)?.starts_with(['.', ')']) {
            return None;
        }
        digits + 1
    };
    content
        .get(width..)?
        .starts_with([' ', '\t'])
        .then_some(width)
}
