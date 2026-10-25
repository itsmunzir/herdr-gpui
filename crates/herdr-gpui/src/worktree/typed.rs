//! The new worktree dialog's branch field formats what is typed into it, so a
//! character Git refuses never reaches the submit check. Only the text being
//! typed or pasted is changed, never the text around it: the caret stays right
//! after the insertion, and nothing already in the field moves under it.
//!
//! A deletion or cut has nothing of its own to change, so when it brings
//! together two characters Git refuses side by side, the text after the caret
//! gives way instead, and the caret stays where the deletion left it.
//!
//! The rules are `git check-ref-format`'s, applied as each character arrives.
//! Letters keep their case and `/` keeps namespaces such as `feat/login`. An
//! ending that more typing can still make valid (a trailing `.`, `/` or
//! `.lock`) is left alone, since every name passes through one while it is
//! typed; the submit check still refuses it if it is sent as it is.

/// Characters Git refuses anywhere in a branch name besides whitespace and
/// control characters. Each becomes a dash, as a space does.
const SEPARATORS: [char; 7] = ['~', '^', ':', '?', '*', '[', '\\'];

/// `inserted` as it may go between `before` and `after`. Whitespace or one of
/// `~^:?*[\` becomes a dash, and so does `{` after `@`. Left out are other
/// control characters, whatever would become a dash at the end of pasted
/// text (a copied line often ends in a newline, a title in a `?`), a dash,
/// dot or slash that would start the name, a dot that would start a
/// component or follow another, a slash after another or after a component
/// ending in `.lock`, and a dash after another, so a typed `Fix - login`
/// reads `Fix-login`. Characters are dropped from the end of the insertion
/// until it joins `after` the same way. Empty when nothing is left.
fn format_typed(before: &str, inserted: &str, after: &str) -> String {
    let mut name = before.to_owned();
    // One character is a key typed, so a space there is the dash wanted next.
    // A paste's leading space still parts it from the word before.
    let inserted = match inserted.chars().nth(1) {
        Some(_) => inserted.trim_end_matches(|ch| separates(ch) || ch.is_control()),
        None => inserted,
    };
    for ch in inserted.chars() {
        push(&mut name, ch);
    }
    while name.len() > before.len() && !joins(&name, after) {
        name.pop();
    }
    name.split_off(before.len())
}

/// An edit replacing what lies between `before` and `after`, as it may leave
/// the field: the text inserted, as `format_typed` keeps it, and the text
/// that then follows it. An insertion is cut back so `after` stays as it was,
/// but a deletion inserts nothing, so `a.x.b` without its `x` would read
/// `a..b`. There `after` gives way by the rules typing follows: a `.`, `/` or
/// `-` doubled collapses to one, `{` after `@` becomes a dash, and a dash,
/// dot or slash that would start the name, a dot that would start a
/// component, or a slash closing a component that ends in `.lock` is left
/// out. The caret goes after the insertion, so it does not move.
pub(crate) fn format_edit(before: &str, inserted: &str, after: &str) -> (String, String) {
    let inserted = format_typed(before, inserted, after);
    let mut name = format!("{before}{inserted}");
    let joined = name.len();
    for ch in after.chars() {
        push(&mut name, ch);
    }
    let after = name.split_off(joined);
    (inserted, after)
}

/// Adds `ch` to the end of `name` as a key typed there would: whitespace or
/// one of `~^:?*[\` as a dash, and nothing when it would form a sequence Git
/// refuses.
fn push(name: &mut String, ch: char) {
    if ch.is_control() && !ch.is_whitespace() {
        return;
    }
    let last = name.chars().next_back();
    let separator = separates(ch) || (ch == '{' && last == Some('@'));
    let ch = if separator { '-' } else { ch };
    // A dash standing in for a separator also stays off a slash.
    if accepts(name, ch) && !(separator && last == Some('/')) {
        name.push(ch);
    }
}

/// Whether `ch` always stands for a dash.
fn separates(ch: char) -> bool {
    ch.is_whitespace() || SEPARATORS.contains(&ch)
}

/// Whether `ch` may follow `name`, the text left of the caret.
fn accepts(name: &str, ch: char) -> bool {
    let last = name.chars().next_back();
    match ch {
        '-' => !matches!(last, None | Some('-')),
        '.' => !matches!(last, None | Some('.' | '/')),
        '/' => !matches!(last, None | Some('/')) && !component(name).ends_with(".lock"),
        _ => true,
    }
}

/// Whether `name` may be followed by `after`, the text right of the caret.
fn joins(name: &str, after: &str) -> bool {
    let (Some(last), Some(next)) = (name.chars().next_back(), after.chars().next()) else {
        return true;
    };
    if matches!(
        (last, next),
        ('/', '/' | '.') | ('.', '.') | ('@', '{') | ('-', '-')
    ) {
        return false;
    }
    // A component the insertion finishes before a slash cannot end in `.lock`.
    match after.split_once('/') {
        Some((rest, _)) => !format!("{}{rest}", component(name)).ends_with(".lock"),
        None => true,
    }
}

/// The last `/`-separated component of `name`.
fn component(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or_default()
}

#[cfg(test)]
mod tests;
