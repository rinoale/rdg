pub mod git;
pub mod rsync;
pub mod tree;
pub mod watcher;

pub(crate) fn truncate_text(mut text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }

    let mut end = max_bytes;

    while !text.is_char_boundary(end) {
        end -= 1;
    }

    text.truncate(end);
    text.push_str("\n\n... truncated ...\n");
    text
}
