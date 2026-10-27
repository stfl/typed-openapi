//! What becomes of a vendor's prose on its way into a generated doc comment.
//!
//! Every description in the document is a doc comment in `types.rs` and
//! `ops.rs`, so this is the one place that decides what a vendor's sentence can
//! do to the crate that compiles it: which of its code blocks rustdoc runs,
//! which of its characters stop the compiler or `rustfmt`, and which of its
//! markup reaches the rendered page.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
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
/// allowance reaches, because a doctest is executed rather than linted. A
/// vendor who means a code block, or who means to hurt, writes the same three
/// one container deeper, and a document is allowed to be either.
///
/// So the prose is made unrunnable where it would otherwise be run, and left
/// alone everywhere else. The generator is what put the prose where rustdoc
/// would execute it, so the generator is where it is made safe; an adopter
/// switching doctests off for the whole crate would be hiding this and taking
/// their own hand-written files with it.
///
/// # How a code block is found
///
/// By asking the parser that rustdoc asks, with the options it passes
/// ([`DIALECT`]). A reading of the text line by line sees a fence only on a line
/// that starts with one, and Markdown puts blocks where no line starts with
/// anything: after the `>` of a block quote, after the marker of a list item,
/// after the `[^note]:` of a footnote. Whatever the parser calls a code block,
/// rustdoc calls one too, so the container it stands in cannot hide it. A fence
/// is made inert by the word after it ([`INERT`]), and a block that has no word
/// to change, because it is indented, has its indentation capped like every
/// other line's.
///
/// # What the page may show
///
/// **A description is text on the page.** Markup in it is the vendor's, and
/// rustdoc puts what the vendor writes into the adopter's HTML as it is, so a
/// `<` outside a code block or a code span is written `\<` and the page shows
/// the character. A code block and a code span are escaped by rustdoc already.
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
/// passed through untouched, and a fence's language is renamed only where
/// rustdoc would read it as Rust, because the word is what a reader's
/// highlighting goes by.
///
/// The edits that do change a byte of the sentence are the ones the compiler
/// and `rustfmt` leave no choice about. A bidirectional control is written as
/// its code point, in a sample too, because the compiler refuses it in a
/// comment and the lint that does so is raised by the lexer, where no `allow`
/// reaches. A carriage return is a line break, because `rustfmt` refuses a bare
/// one and Markdown reads one as a break.
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

/// The Markdown dialect rustdoc reads a doc comment in.
///
/// These are the options of `main_body_opts` in `rustc_resolve`, which
/// `rustdoc --test` hands the parser when it looks for code blocks. Tables and
/// footnotes are in it because each makes a container, and a block in a
/// footnote definition is a block rustdoc finds.
const DIALECT: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_FOOTNOTES)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_SMART_PUNCTUATION);

/// The characters that change the direction of text, which are the ones rustc
/// refuses in a comment (`text_direction_codepoint_in_comment`): the three
/// embeddings and overrides that open a run, the isolates, and the two that end
/// one.
///
/// The marks (`U+200E`, `U+200F` and `U+061C`) are left as they are. They do
/// not reorder a run, the compiler accepts them, and a description in Hebrew or
/// Arabic may be written with them.
const BIDI: [char; 9] = [
    '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}',
    '\u{2069}',
];

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

/// The language named on a fence rustdoc will not compile.
///
/// Any word it does not recognise does, and this one says what the block is.
const INERT: &str = "text";

/// The words rustdoc gives a meaning to in the info string of a fence, bar the
/// two families that [`names_a_language`] reads by their start.
///
/// A fence carrying one of them — or carrying nothing — is a block rustdoc may
/// compile and run, so a vendor who wrote `rust` over a line of pseudocode has
/// written a doctest without meaning to. `custom` does not make one, and is
/// here because the word is rustdoc's to give a meaning to.
const RUSTDOC: [&str; 8] = [
    "compile_fail",
    "custom",
    "ignore",
    "no_run",
    "rust",
    "should_panic",
    "standalone_crate",
    "test_harness",
];

/// `prose` with nothing in it that rustdoc would compile, that stops the
/// compiler or `rustfmt`, or that the rendered page would show as markup.
///
/// The order of the steps is the design. The characters come first, because
/// every later step reads lines. The margins come next, and then the shared
/// indentation, because `rustfmt` and rustdoc change the text on its way to the
/// parser and the parser has to be asked about the text that rustdoc asks it
/// about. The line steps follow, because they turn what the vendor indented into
/// what the vendor meant. The parser comes last, and it is asked again after
/// each change it asks for ([`settled`]).
fn unrunnable(prose: &str) -> String {
    let settled = settled(tamed(&flush(&margins(&plain(prose)))));
    if !settled.contains('\n') {
        // One line is one `///`, where nothing is indented and there is
        // nothing to strip.
        return settled;
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
    let opening = if starred(&settled) { "\n\n" } else { "\n" };
    format!("{opening}{settled}\n")
}

/// Does rustc take a column of stars out of `text`, written as a `/* */`
/// comment, along with the white space in front of it?
///
/// It does when every line after the first, but for the empty ones at either
/// end, starts with white space and then a star in one column. The first line
/// is left out of the count unless a star starts it. A thematic break written
/// `***`, or a line that opens with `**bold**`, is such a line when it is the
/// only one after the first.
///
/// The white space that goes is the column of the item and the column the star
/// stands in, and the first line has only the first of them to lose, so rustc
/// leaves it standing at the column of its item: four columns in, for a field,
/// which is a code block. A comment opened on a second blank line counts its
/// first line with the rest, and that line either has a star in the same
/// column or ends the question.
///
/// This is `get_horizontal_trim` of rustc, asked of the lines as they stand in
/// the comment. The column of the item is in front of each of them, so it takes
/// no part in the answer.
fn starred(text: &str) -> bool {
    let lines: Vec<&str> = text.split('\n').collect();
    let skipped = usize::from(
        !lines
            .first()
            .is_some_and(|line| line.trim_start().starts_with('*')),
    );
    let counted = lines.get(skipped..).unwrap_or_default();
    let (Some(first), Some(last)) = (
        counted.iter().position(|line| !line.trim().is_empty()),
        counted.iter().rposition(|line| !line.trim().is_empty()),
    ) else {
        return false;
    };
    let mut column = None;
    for line in counted.get(first..=last).unwrap_or_default() {
        column = star(line, column);
        if column.is_none() {
            return false;
        }
    }
    true
}

/// The column of the star that `line` starts with, after white space or other
/// stars, when it stands in `column` or when no column is known yet.
fn star(line: &str, column: Option<usize>) -> Option<usize> {
    for (at, c) in line.chars().enumerate() {
        if column.is_some_and(|star| at > star) || !matches!(c, '*' | ' ' | '\t') {
            return None;
        }
        if c == '*' {
            return column.is_none_or(|star| star == at).then_some(at);
        }
    }
    None
}

/// `prose` with every line break a line feed and every bidirectional control
/// written as its code point.
///
/// A carriage return and the line feed after it are one break, and a carriage
/// return alone is one. The code point is written as Rust writes it
/// (`\u{202b}`), so the comment shows what the vendor's text held.
fn plain(prose: &str) -> String {
    let mut out = String::with_capacity(prose.len());
    let mut chars = prose.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            chars.next_if_eq(&'\n');
            out.push('\n');
        } else if BIDI.contains(&c) {
            out.extend(c.escape_unicode());
        } else {
            out.push(c);
        }
    }
    out
}

/// `prose` with the white space at the ends of each line as `rustfmt` leaves it.
///
/// `rustfmt` indents the body of a `/* */` comment to its item and, line by
/// line, keeps the spaces a line starts with, writes each tab among them as four
/// spaces, deletes any other white space where the line starts, such as a
/// no-break space, and deletes the white space at its end.
/// What rustdoc reads is that text. A line that starts with a tab, or with a
/// no-break space and four spaces, is not a code block to the parser here and is
/// one to rustdoc, so the text is written as `rustfmt` writes it before the
/// parser reads it, and `rustfmt` has nothing left to change.
///
/// A line with nothing but white space in it is empty.
fn margins(prose: &str) -> String {
    prose
        .split('\n')
        .map(|line| {
            let content = line.trim_start_matches([' ', '\t']);
            let indent = line.len() - content.len();
            let columns: usize = line
                .bytes()
                .take(indent)
                .map(|byte| if byte == b'\t' { TAB } else { 1 })
                .sum();
            let text = content.trim();
            if text.is_empty() {
                String::new()
            } else {
                format!("{}{text}", " ".repeat(columns))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `prose` without the indentation that all of its lines share.
///
/// rustdoc takes it off: the fewest spaces that a line with a word in it starts
/// with, taken from every such line, and `rustfmt` has indented each of them by
/// the column of the item, which is the same for all. The parser is asked about
/// the text without the indentation, because that is the text rustdoc asks it
/// about, and a block can be code to the one and not to the other: a line that
/// is six columns in beside one that is two is code at four.
///
/// It reads the text [`margins`] returns, whose lines start with spaces only
/// and whose empty lines have none.
fn flush(prose: &str) -> String {
    let shared = prose
        .split('\n')
        .filter(|line| !line.is_empty())
        .map(|line| line.len() - line.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0);
    prose
        .split('\n')
        .map(|line| line.get(shared..).unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `prose` with no line indented deeply enough to open a code block, no list
/// item hanging its content far enough to open one, and no bullet rustc will
/// eat.
///
/// A line is capped, then unhung, then dashed, and the first step's place in
/// that order is the rule: Markdown reads a fence at three columns or fewer, so
/// an indented one is not a fence at all but the start of an indented code
/// block — capping it first is what turns it into the fence the vendor meant,
/// rather than leaving a block whose first line happens to be three backticks.
/// The two list steps cannot disturb that, because a run of backticks is not a
/// list marker.
///
/// A line inside a fence is left alone. The parser says which those are, in
/// the text as the vendor wrote it: a fence that is already a fence, in
/// whatever container, holds a sample that is the vendor's own, and the fence
/// above it already says nobody will run it. An indented fence is not a fence
/// yet, so its lines are capped like any others, and it is read as the fence it
/// has become when [`settled`] asks the parser again.
fn tamed(prose: &str) -> String {
    let fenced = fenced_lines(prose);
    prose
        .split('\n')
        .enumerate()
        .map(|(at, line)| {
            if fenced.binary_search(&at).is_ok() {
                line.to_owned()
            } else {
                dashed(&unhung(&capped(line)))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The numbers of the lines that a fenced code block covers, fences included, in
/// order.
fn fenced_lines(prose: &str) -> Vec<usize> {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(prose.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    let line_of = |offset: usize| starts.partition_point(|&start| start <= offset) - 1;
    let mut lines = Vec::new();
    for (event, range) in Parser::new_ext(prose, DIALECT).into_offset_iter() {
        if let Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_))) = event {
            lines.extend(line_of(range.start)..=line_of(range.end.saturating_sub(1)));
        }
    }
    lines.dedup();
    lines
}

/// One change to a description: the bytes at `at` become `with`.
struct Edit {
    at: Range<usize>,
    with: &'static str,
}

/// `text` with the changes that the parser asks for, until it asks for none.
///
/// A change can make the next one necessary. Capping the indentation of a block
/// in a quote leaves a fence that the vendor had indented; escaping the `<`
/// that opened an HTML block leaves the lines it covered to be read as
/// Markdown, and one of them may open a fence; and a block that was code is
/// prose once its indentation is taken, with the `<` it held. So the parser
/// reads the text again after every round, and the text is finished when a
/// reading finds nothing to change.
///
/// It finishes, because a round lowers one of three counts, and the counts are
/// ranked: the `<` that no backslash escapes, the white space in the text, and
/// the fences that rustdoc reads as Rust. A change never raises a count that is
/// ranked above its own, and it may raise one below, since taking code apart
/// can show a `<` or a fence. Escaping lowers the first and writes no white
/// space, taking white space away lowers the second and writes no `<`, and
/// naming a language lowers the third and writes neither.
///
/// The argument is about this module's changes and a parser that reads text the
/// way this module expects. A parser that does not is not a reason to hang a
/// bless, so a description that has not settled in [`ROUNDS`] readings is kept
/// whole as a [`sample`].
fn settled(mut text: String) -> String {
    for _ in 0..ROUNDS {
        let edits = found(&text).edits;
        if edits.is_empty() {
            return text;
        }
        text = applied(&text, edits);
    }
    sample(&text)
}

/// How many readings [`settled`] makes before it gives up.
///
/// A description needs one for each layer that a change takes off: a block in a
/// quote in a list is three. Eight times that is a description nobody wrote.
const ROUNDS: usize = 24;

/// `text` as a sample in a fence that names [`INERT`].
///
/// A sample is the one shape in which nothing can be run and nothing is markup,
/// whatever the text holds, so it is where a description goes that the parser
/// reads as nothing this module expects. The fence is longer than any run of
/// backticks in the text, so no line of the text closes it.
fn sample(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}{INERT}\n{text}\n{fence}")
}

/// `text` with `edits` made. The changes of one reading do not overlap; one that
/// did would be left for the next reading, which finds it again.
fn applied(text: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|edit| edit.at.start);
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    for edit in edits {
        if edit.at.start < from {
            continue;
        }
        out.push_str(text.get(from..edit.at.start).unwrap_or_default());
        out.push_str(edit.with);
        from = edit.at.end;
    }
    out.push_str(text.get(from..).unwrap_or_default());
    out
}

/// What one reading of a description found that has to change.
#[derive(Default)]
struct Found {
    edits: Vec<Edit>,
    /// Where a `<` stays as written in this reading, in the order the text has
    /// them: the code blocks and the code spans, which rustdoc shows as text,
    /// and the lines of an HTML block, which this reading escapes by their
    /// first `<` and no more. The rest of such a line is read again as the
    /// Markdown it is once nothing opens a block, and a code span in it is
    /// then a code span.
    kept: Vec<Range<usize>>,
}

/// Read `text` as rustdoc does, and say what in it has to change.
///
/// Every kind of event is named, so that a kind the parser gains is a worklist
/// for this function and not a case it passes over.
fn found(text: &str) -> Found {
    let mut found = Found::default();
    let mut indented = false;
    for (event, range) in Parser::new_ext(text, DIALECT).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => indented = found.block(text, &kind, range),
            Event::End(TagEnd::CodeBlock) => indented = false,
            Event::Text(_) if indented => found.edits.extend(dedented(text, range)),
            Event::Code(_) => found.kept.push(range),
            Event::Html(_) => {
                found.edits.extend(opening(text, range.clone()));
                found.kept.push(range);
            }
            Event::Start(_)
            | Event::End(_)
            | Event::Text(_)
            | Event::InlineHtml(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::FootnoteReference(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::Rule
            | Event::TaskListMarker(_) => {}
        }
    }
    let angles = angles(text, &found.kept);
    found.edits.extend(angles);
    found
}

impl Found {
    /// Note a code block, which is shown as text; rename its fence if rustdoc
    /// would read it as Rust. Answer whether the block is an indented one.
    fn block(&mut self, text: &str, kind: &CodeBlockKind<'_>, range: Range<usize>) -> bool {
        self.kept.push(range.clone());
        match kind {
            CodeBlockKind::Fenced(info) => {
                if !names_a_language(info) {
                    self.edits.extend(inert(text, range.start));
                }
                false
            }
            CodeBlockKind::Indented => true,
        }
    }
}

/// Does the info string of a fence name a language that rustdoc leaves alone?
///
/// Rustdoc reads an info string as words, split at a comma, a space and a tab,
/// and a block is Rust when it has no word or has one that means Rust to it. A
/// word it does not know, such as `json`, is another language's name, and the
/// block is not Rust unless a word of Rust's follows. That rule has more shapes
/// than a vendor writes, and rustdoc adds words over time, so the info string is
/// kept only where it is the plain case: every word is made of
/// the letters, digits and signs that a language is named with, and none is a
/// word that rustdoc gives a meaning to. Anything else is renamed. A `{.json}`
/// or a `key=value` is a syntax of rustdoc's own, `ignore-wasm32` skips a
/// target and runs everywhere else, and `r&#117;st` is `rust` once Markdown has
/// read it, which is why the word that is tested is the one the parser hands
/// over and not the text of the line.
fn names_a_language(info: &str) -> bool {
    let mut words = info
        .split([',', ' ', '\t'])
        .filter(|word| !word.is_empty())
        .peekable();
    words.peek().is_some() && words.all(language)
}

/// Is `word` the name of a language, and a word rustdoc has no meaning for?
fn language(word: &str) -> bool {
    word.starts_with(|c: char| c.is_ascii_alphabetic())
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '#'))
        && !RUSTDOC.contains(&word)
        && !word.starts_with("ignore-")
        && !word.starts_with("edition")
        && !word
            .strip_prefix('E')
            .is_some_and(|code| code.len() == 4 && code.parse::<u32>().is_ok())
}

/// The change that gives the fence opening at `start` the word [`INERT`] in
/// place of whatever follows the fence on its line.
///
/// The parser puts the start of a fenced block at its fence, however deep in a
/// container the block stands, so the info string is what remains of the line.
/// Nothing is changed if the text at `start` is no fence, which the parser does
/// not report.
fn inert(text: &str, start: usize) -> Option<Edit> {
    let line = text.get(start..)?.split('\n').next()?;
    let mark = line.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let fence = line.len() - line.trim_start_matches(mark).len();
    Some(Edit {
        at: start + fence..start + line.len(),
        with: INERT,
    })
}

/// The change that takes a line of an indented code block out of it, for the
/// text of the line at `line`.
///
/// An indented block has no word to rename, so the indentation is what changes:
/// the whitespace that follows the four columns of indentation goes, and so
/// does one character of the four. The line is then three columns in, from its
/// container, and is a paragraph or an item and not code. That is the cap that
/// [`capped`] makes for a line that stands in no container, and it is made by
/// removing characters, whatever a tab was worth.
///
/// Nothing is changed for a blank line or for the spaces a tab stands for that
/// the parser reports with no place in the text, since neither has white space
/// in front of it in the text.
fn dedented(text: &str, line: Range<usize>) -> Option<Edit> {
    let code = text.get(line.clone())?;
    let before = line.start.checked_sub(1)?;
    let indentation = matches!(text.as_bytes().get(before), Some(b' ' | b'\t'));
    let extra = code.len() - code.trim_start_matches([' ', '\t']).len();
    indentation.then_some(Edit {
        at: before..line.start + extra,
        with: "",
    })
}

/// The change that writes the first `<` of a line of an HTML block as `\<`,
/// when the backslash is not there already.
fn opening(text: &str, line: Range<usize>) -> Option<Edit> {
    let at = line.start + text.get(line)?.find('<')?;
    (!escaped(text, at)).then_some(backslash(at))
}

/// The changes that write each `<` outside the ranges in `kept` as `\<`.
///
/// A `<` that a backslash escapes already is left.
fn angles(text: &str, kept: &[Range<usize>]) -> Vec<Edit> {
    text.match_indices('<')
        .map(|(at, _)| at)
        .filter(|&at| !within(kept, at) && !escaped(text, at))
        .map(backslash)
        .collect()
}

/// The change that puts a backslash before the character at `at`.
fn backslash(at: usize) -> Edit {
    Edit {
        at: at..at,
        with: "\\",
    }
}

/// Does one of `ranges`, which are in order and do not overlap, hold `at`?
fn within(ranges: &[Range<usize>], at: usize) -> bool {
    let after = ranges.partition_point(|range| range.end <= at);
    ranges.get(after).is_some_and(|range| range.start <= at)
}

/// Does a backslash escape the character at `at`? An odd run of them does, and
/// an even one is a backslash that escapes another.
fn escaped(text: &str, at: usize) -> bool {
    let run = text
        .get(..at)
        .unwrap_or_default()
        .bytes()
        .rev()
        .take_while(|&byte| byte == b'\\')
        .count();
    run % 2 == 1
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

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]
mod tests {
    use super::*;

    /// What the parser finds in `text`: each fenced block as the word after its
    /// fence and the text inside it, and anything else as `other`.
    fn blocks(text: &str) -> Vec<(String, String)> {
        let mut found = Vec::new();
        let mut inside: Option<(String, String)> = None;
        for event in Parser::new_ext(text, DIALECT) {
            if let Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) = &event {
                inside = Some((info.to_string(), String::new()));
            } else if let (Event::Text(code), Some((_, held))) = (&event, inside.as_mut()) {
                held.push_str(code);
            } else if matches!(event, Event::End(TagEnd::CodeBlock)) {
                found.extend(inside.take());
            } else {
                found.push(("other".to_owned(), format!("{event:?}")));
            }
        }
        found
    }

    #[test]
    fn a_sample_is_one_block_that_holds_the_text_whatever_it_is() {
        let texts = [
            "plain",
            "```\nrust\n```",
            "````\nfour\n````\n```\nthree\n```",
            "~~~\ntilde\n~~~",
            "> quoted\n\n    indented\n\n<div>html</div>",
            "``````````",
            "",
        ];
        for text in texts {
            let held = blocks(&sample(text));
            assert_eq!(
                held,
                [(INERT.to_owned(), format!("{text}\n"))],
                "the sample of {text:?}"
            );
        }
    }

    #[test]
    fn the_word_after_a_fence_is_what_is_renamed_and_nothing_that_is_no_fence_is_touched() {
        let edit = inert("- ```rust,ignore\n  code", 2).expect("a fence at 2");
        assert_eq!(edit.at, 5..16);
        assert_eq!(edit.with, INERT);
        let edit = inert("   ~~~~  {.json}", 3).expect("a fence at 3");
        assert_eq!(edit.at, 7..16);
        let bare = inert("```", 0).expect("a fence with no word");
        assert_eq!(bare.at, 3..3);
        assert!(inert("rust", 0).is_none(), "no fence starts the text");
        assert!(inert("```", 9).is_none(), "nothing is at the start");
    }

    #[test]
    fn only_white_space_is_taken_from_in_front_of_a_line_of_an_indented_block() {
        let edit = dedented(">     code", 6..10).expect("white space stands in front");
        assert_eq!(edit.at, 5..6);
        let edit = dedented(">\t\t  code", 4..9).expect("a space stands in front");
        assert_eq!(
            edit.at,
            3..5,
            "the space in front and the one the text starts with"
        );
        assert!(
            dedented(">code", 1..5).is_none(),
            "a `>` is not white space"
        );
        assert!(dedented("code", 0..4).is_none(), "nothing stands in front");
    }

    #[test]
    fn a_fence_is_longer_than_every_run_of_backticks_in_the_text() {
        let sampled = sample("a ````` b `` c");
        assert!(sampled.starts_with("``````text\n"), "{sampled}");
        assert!(sampled.ends_with("\n``````"), "{sampled}");
        assert!(sample("none").starts_with("```text\n"));
    }
}
