// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

#[cfg(test)]
mod tests {
    use crate::boundaries::imports;
    use crate::index::symbols;

    #[test]
    fn package_imports_yield_the_stripped_path() {
        let text = "import 'package:umarell/features/chat/chat_service.dart';\n\
                    export 'package:umarell/core/result.dart';\n\
                    import \"package:flutter/material.dart\";\n\
                    import 'package:provider/provider.dart' as p;\n\
                    import 'package:umarell/models/user.dart' show User, Role;\n\
                    export 'package:umarell/models/role.dart' hide Internal;\n";
        assert_eq!(
            imports("lib/features/chat/chat_view.dart", text),
            vec![
                "umarell/features/chat/chat_service",
                "umarell/core/result",
                "flutter/material",
                "provider/provider",
                "umarell/models/user",
                "umarell/models/role",
            ]
        );
    }

    /// A `dart:` spec names an SDK builtin, the `node:` precedent, and a
    /// relative spec names a file rather than a package, the JS relative
    /// precedent. Neither can resolve to a declared module, and both would
    /// hand the resolver leading segments (`dart`, `..`, a bare file stem)
    /// that can collide with a module whose leaf happens to match.
    #[test]
    fn sdk_and_relative_imports_are_skipped() {
        let text = "import 'dart:async';\n\
                    import 'dart:io' as io;\n\
                    import 'chat_state.dart';\n\
                    import '../core/result.dart';\n\
                    import './widgets/bubble.dart';\n";
        let found = imports("lib/features/chat/chat_view.dart", text);
        assert!(found.is_empty(), "{found:?}");
    }

    /// `part` and `part of` splice one file into another's declaration
    /// space. They are file inclusion, not dependency: the two halves are
    /// the same library, so a boundary edge between them would be an edge
    /// from a module to itself.
    #[test]
    fn part_directives_are_not_imports() {
        let text = "part 'chat_service.g.dart';\n\
                    part of 'chat_service.dart';\n\
                    part 'package:umarell/core/result.g.dart';\n";
        let found = imports("lib/features/chat/chat_service.dart", text);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn top_level_declarations_are_extracted() {
        let text = "class ChatService {\n\
                    abstract class Repository {\n\
                    sealed class Result {\n\
                    enum Status { idle, busy }\n\
                    mixin Logging {\n\
                    extension StringX on String {\n\
                    typedef Json = Map<String, dynamic>;\n\
                    void main() {\n\
                    Future<void> load(String id) async {\n\
                    Map<String, int> tally(List<String> rows) {\n\
                    final cache = compute(1);\n\
                      Widget indented(BuildContext context) {\n\
                    }\n";
        let found = symbols("lib/features/chat/chat_service.dart", text);
        let pairs: Vec<(&str, &str)> = found
            .iter()
            .map(|symbol| (symbol.kind.as_str(), symbol.name.as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("class", "ChatService"),
                ("class", "Repository"),
                ("class", "Result"),
                ("enum", "Status"),
                ("mixin", "Logging"),
                ("extension", "StringX"),
                ("typedef", "Json"),
                ("fn", "main"),
                ("fn", "load"),
                ("fn", "tally"),
            ]
        );
    }

    /// A leading underscore is Dart's privacy rule, not a naming
    /// convention: `_ChatViewState` is library-private by language
    /// definition, so it is no more part of the file's public surface than
    /// a Rust item without `pub`.
    #[test]
    fn underscore_private_names_are_skipped() {
        let text = "class _ChatViewState {\n\
                    enum _Mode { a }\n\
                    typedef _Json = Map<String, dynamic>;\n\
                    void _helper() {\n\
                    class Public {\n";
        let found = symbols("lib/features/chat/chat_view.dart", text);
        let names: Vec<&str> = found.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, vec!["Public"]);
    }
}
