use camino::Utf8Path;
use if_chain::if_chain;
use itertools::Itertools as _;
use proc_macro2::{LineColumn, TokenStream, TokenTree};
use std::collections::BTreeMap;
use syn::{spanned::Spanned as _, Attribute, File, Item, ItemMod, Lit, Meta, MetaNameValue};

pub(crate) fn expand_mods(src_path: &Utf8Path) -> Result<String, String> {
    return expand_mods(src_path, 0);

    fn expand_mods(src_path: &Utf8Path, depth: usize) -> Result<String, String> {
        let code = &read_file(src_path)?;
        let File { items, .. } =
            syn::parse_file(code).map_err(|e| format!("could not parse `{}`: {}", src_path, e))?;

        let replacements = items
            .into_iter()
            .flat_map(|item| match item {
                Item::Mod(ItemMod {
                    attrs,
                    ident,
                    content: None,
                    semi,
                    ..
                }) => Some((attrs, ident, semi)),
                _ => None,
            })
            .map(|(attrs, ident, semi)| {
                let paths = if let Some(path) = attrs
                    .iter()
                    .flat_map(Attribute::parse_meta)
                    .flat_map(|meta| match meta {
                        Meta::NameValue(name_value) => Some(name_value),
                        _ => None,
                    })
                    .filter(|MetaNameValue { path, .. }| {
                        matches!(path.get_ident(), Some(i) if i == "path")
                    })
                    .find_map(|MetaNameValue { lit, .. }| match lit {
                        Lit::Str(s) => Some(s.value()),
                        _ => None,
                    }) {
                        vec![src_path.with_file_name("").join(path)]
                    } else if depth == 0 || src_path.file_name() == Some("mod.rs") {
                        vec![
                            src_path
                                .with_file_name(&ident.to_string())
                                .with_extension("rs"),
                            src_path.with_file_name(&ident.to_string()).join("mod.rs"),
                        ]
                    } else {
                        vec![
                            src_path
                                .with_extension("")
                                .with_file_name(&ident.to_string())
                                .with_extension("rs"),
                            src_path
                                .with_extension("")
                                .with_file_name(&ident.to_string())
                                .join("mod.rs"),
                        ]
                    };

                if let Some(path) = paths.iter().find(|p| p.exists()) {
                    let start = semi.span().start();
                    let end = semi.span().end();
                    let content = expand_mods(&path, depth + 1)?;
                    let content = indent_code(&content, depth + 1);
                    let content = format!(" {{\n{}{}}}", content, "    ".repeat(depth + 1));
                    Ok(((start, end), content))
                } else {
                    Err(format!("one of {:?} does not exist", paths))
                }
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;

        Ok(replace_ranges(code, replacements))
    }

    fn read_file(path: &Utf8Path) -> Result<String, String> {
        xshell::read_file(path).map_err(|e| e.to_string())
    }

    fn indent_code(code: &str, n: usize) -> String {
        let is_safe_to_indent = code.parse::<TokenStream>().map_or(false, |token_stream| {
            !token_stream.into_iter().any(|tt| {
                matches!(
                    tt, TokenTree::Literal(lit)
                    if lit.span().start().line != lit.span().end().line
                )
            })
        });

        if is_safe_to_indent {
            code.lines()
                .map(|line| match line {
                    "" => "\n".to_owned(),
                    line => format!("{}{}\n", "    ".repeat(n), line),
                })
                .join("")
        } else {
            code.to_owned()
        }
    }

    fn replace_ranges(
        code: &str,
        replacements: BTreeMap<(LineColumn, LineColumn), String>,
    ) -> String {
        let replacements = replacements.into_iter().collect::<Vec<_>>();
        let mut replacements = &*replacements;
        let mut skip_until = None;
        let mut ret = "".to_owned();
        let mut lines = code.trim_end().split('\n').enumerate().peekable();
        while let Some((i, s)) = lines.next() {
            for (j, c) in s.chars().enumerate() {
                if_chain! {
                    if let Some(((start, end), replacement)) = replacements.get(0);
                    if (i, j) == (start.line - 1, start.column);
                    then {
                        ret += replacement;
                        if start == end {
                            ret.push(c);
                        } else {
                            skip_until = Some(*end);
                        }
                        replacements = &replacements[1..];
                    } else {
                        if !matches!(skip_until, Some(LineColumn { line, column }) if (i, j) < (line - 1, column)) {
                            ret.push(c);
                            skip_until = None;
                        }
                    }
                }
            }
            while let Some(((start, end), replacement)) = replacements.get(0) {
                if i == start.line - 1 {
                    ret += replacement;
                    if start < end {
                        skip_until = Some(*end);
                    }
                    replacements = &replacements[1..];
                } else {
                    break;
                }
            }
            if lines.peek().is_some() || code.ends_with('\n') {
                ret += "\n";
            }
        }

        debug_assert!(syn::parse_file(&code).is_ok());

        ret
    }
}
