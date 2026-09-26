// Adds one object property to JSON/JSONC without reformatting unrelated content.
using System;
using System.Collections.Generic;
using System.Text;
public static class AutoGuiJson {
    sealed class Node {
        public int Start, End;
        public bool Object;
        public Dictionary<string, Node> Members = new Dictionary<string, Node>(StringComparer.Ordinal);
    }
    sealed class Parser {
        readonly string text;
        int pos;
        public Parser(string input) { text = input; }
        void Skip() {
            for (;;) {
                while (pos < text.Length && char.IsWhiteSpace(text[pos])) pos++;
                if (pos + 1 >= text.Length || text[pos] != '/') return;
                if (text[pos + 1] == '/') { pos += 2; while (pos < text.Length && text[pos] != '\n') pos++; }
                else if (text[pos + 1] == '*') { int end = text.IndexOf("*/", pos + 2, StringComparison.Ordinal); if (end < 0) throw new FormatException("Unclosed comment"); pos = end + 2; }
                else return;
            }
        }
        void Need(char c) { Skip(); if (pos >= text.Length || text[pos++] != c) throw new FormatException("Invalid JSONC syntax"); }
        string StringValue() {
            Need('"'); var result = new StringBuilder();
            while (pos < text.Length) {
                char c = text[pos++];
                if (c == '"') return result.ToString();
                if (c < 32) throw new FormatException("Control character in string");
                if (c == '\\') {
                    if (pos >= text.Length) break;
                    c = text[pos++];
                    switch (c) {
                        case '"': case '\\': case '/': break;
                        case 'b': c = '\b'; break; case 'f': c = '\f'; break;
                        case 'n': c = '\n'; break; case 'r': c = '\r'; break; case 't': c = '\t'; break;
                        case 'u': if (pos + 4 > text.Length) throw new FormatException("Invalid unicode escape"); c = (char)Convert.ToInt32(text.Substring(pos, 4), 16); pos += 4; break;
                        default: throw new FormatException("Invalid escape");
                    }
                }
                result.Append(c);
            }
            throw new FormatException("Unclosed string");
        }
        Node Value(int depth) {
            if (depth > 100) throw new FormatException("Config nesting exceeds 100");
            Skip(); if (pos >= text.Length) throw new FormatException("Missing value");
            var node = new Node { Start = pos };
            if (text[pos] == '{' || text[pos] == '[') {
                node.Object = text[pos++] == '{'; char close = node.Object ? '}' : ']'; Skip();
                while (pos < text.Length && text[pos] != close) {
                    if (node.Object) {
                        string key = StringValue(); Need(':'); Node value = Value(depth + 1);
                        if (node.Members.ContainsKey(key)) throw new FormatException("Duplicate property: " + key);
                        node.Members.Add(key, value);
                    } else Value(depth + 1);
                    Skip(); if (pos < text.Length && text[pos] == close) break;
                    Need(','); Skip();
                }
                Need(close);
            } else if (text[pos] == '"') StringValue();
            else {
                int start = pos;
                while (pos < text.Length && !char.IsWhiteSpace(text[pos]) && ",}]/".IndexOf(text[pos]) < 0) pos++;
                string value = text.Substring(start, pos - start);
                if (value != "true" && value != "false" && value != "null" && !System.Text.RegularExpressions.Regex.IsMatch(value, @"^-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?$")) throw new FormatException("Invalid primitive");
            }
            node.End = pos; return node;
        }
        public Node Parse() { var n = Value(0); Skip(); if (pos != text.Length) throw new FormatException("Trailing config content"); return n; }
    }
    public static string AddServer(string text, string container, string name, string value) {
        var root = new Parser(text).Parse();
        if (!root.Object) throw new FormatException("Config root must be an object");
        new Parser(value).Parse();
        Node destination;
        string key;
        if (root.Members.TryGetValue(container, out destination)) {
            if (!destination.Object) throw new FormatException(container + " must be an object");
            if (destination.Members.ContainsKey(name)) throw new InvalidOperationException("MCP server already exists: " + name);
            key = name;
        } else { destination = root; key = container; value = "{\"" + name + "\":" + value + "}"; }
        // container/name are fixed installer identifiers, never arbitrary user text.
        string insertion = "\n  \"" + key + "\": " + value + (destination.Members.Count == 0 ? "\n" : ",\n");
        string updated = text.Insert(destination.Start + 1, insertion);
        new Parser(updated).Parse();
        return updated;
    }
}
