// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

#[cfg(test)]
mod tests {
    use crate::boundaries::imports;
    use crate::index::symbols;

    #[test]
    fn using_directives_yield_the_full_namespace_path() {
        let text = "using Ummarell.Api.Services;\n\
                    global using System.Text.Json;\n\
                    namespace Ummarell.Api.Web;\n";
        assert_eq!(
            imports("src/Ummarell.Api/Home.cs", text),
            vec!["Ummarell.Api.Services", "System.Text.Json"]
        );
    }

    #[test]
    fn using_static_and_alias_yield_the_type_path() {
        let text = "using static System.Math;\nusing Db = Ummarell.Api.Data;\n";
        assert_eq!(
            imports("src/Ummarell.Api/Home.cs", text),
            vec!["System.Math", "Ummarell.Api.Data"]
        );
    }

    #[test]
    fn a_using_statement_is_not_an_import() {
        let text = "using (var f = File.Open(path))\n\
                    {\n\
                    }\n\
                    using var scope = provider.CreateScope();\n\
                    await using var conn = OpenAsync();\n";
        let found = imports("src/Ummarell.Api/Home.cs", text);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn public_type_declarations_are_extracted() {
        let text = "public class Foo\n    \
                    public sealed record Bar<T>\n\
                    public enum Baz\n\
                    public record struct Point\n\
                    public interface IThing\n\
                    public void Run()\n\
                    public int Count { get; set; }\n";
        let found = symbols("src/Ummarell.Api/Home.cs", text);
        let pairs: Vec<(&str, &str)> = found
            .iter()
            .map(|symbol| (symbol.kind.as_str(), symbol.name.as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("class", "Foo"),
                ("record", "Bar"),
                ("enum", "Baz"),
                ("record", "Point"),
                ("interface", "IThing"),
            ]
        );
    }

    #[test]
    fn non_public_types_are_skipped() {
        let text = "internal class X\nclass Y\nprivate sealed class Z\n";
        let found = symbols("src/Ummarell.Api/Home.cs", text);
        assert!(found.is_empty(), "{found:?}");
    }
}
