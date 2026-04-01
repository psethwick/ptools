use crate::extension_trait::{Extension, ExtensionError, ExtensionItem, ExtensionMetadata};
use crate::modes;
use async_trait::async_trait;
use std::fmt;

pub struct CalculatorExtension {
    metadata: ExtensionMetadata,
}

impl fmt::Debug for CalculatorExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CalculatorExtension")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl Default for CalculatorExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl CalculatorExtension {
    pub fn new() -> Self {
        Self {
            metadata: ExtensionMetadata {
                name: modes::CALCULATOR.to_string(),
                version: "1.0.0".to_string(),
                description: Some("Evaluate mathematical expressions".to_string()),
                author: None,
                language: crate::extension_trait::ExtensionLanguage::JavaScript,
                entry_point: "calculator_extension.rs".to_string(),
                permissions: vec![],
                auto_load: true,
                title: None,
                preferences: vec![],
                is_development: false,
            },
        }
    }

    /// Evaluate a mathematical expression string. Returns `None` for invalid input.
    fn evaluate(expr: &str) -> Option<f64> {
        let trimmed = expr.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut parser = Parser::new(trimmed);
        parser
            .parse_expr()
            .filter(|_| parser.pos == parser.input.len())
    }

    fn format_number(n: f64) -> String {
        if n.fract() == 0.0 && n.abs() < 1e15 {
            format!("{}", n as i64)
        } else {
            // Trim trailing zeros from decimal representation
            let s = format!("{:.10}", n);
            let s = s.trim_end_matches('0').trim_end_matches('.');
            s.to_string()
        }
    }
}

// ── Recursive descent parser ─────────────────────────────────────────────────
//
//   expr   := term   (('+' | '-') term)*
//   term   := factor (('*' | '/') factor)*
//   factor := ['-'] factor | '(' expr ')' | number
//   number := digit+ ['.' digit+]

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            input: s.as_bytes(),
            pos: 0,
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.input.get(self.pos).copied()
    }

    fn parse_number(&mut self) -> Option<f64> {
        self.skip_ws();
        let start = self.pos;
        while self.pos < self.input.len()
            && (self.input[self.pos].is_ascii_digit() || self.input[self.pos] == b'.')
        {
            self.pos += 1;
        }
        if self.pos == start {
            return None;
        }
        std::str::from_utf8(&self.input[start..self.pos])
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
    }

    fn parse_factor(&mut self) -> Option<f64> {
        match self.peek()? {
            b'-' => {
                self.pos += 1;
                Some(-self.parse_factor()?)
            }
            b'(' => {
                self.pos += 1;
                let val = self.parse_expr()?;
                self.skip_ws();
                if self.peek()? != b')' {
                    return None;
                }
                self.pos += 1;
                Some(val)
            }
            _ => self.parse_number(),
        }
    }

    fn parse_term(&mut self) -> Option<f64> {
        let mut val = self.parse_factor()?;
        loop {
            match self.peek() {
                Some(b'*') => {
                    self.pos += 1;
                    val *= self.parse_factor()?;
                }
                Some(b'/') => {
                    self.pos += 1;
                    let d = self.parse_factor()?;
                    if d == 0.0 {
                        return None;
                    }
                    val /= d;
                }
                _ => break,
            }
        }
        Some(val)
    }

    fn parse_expr(&mut self) -> Option<f64> {
        let mut val = self.parse_term()?;
        loop {
            match self.peek() {
                Some(b'+') => {
                    self.pos += 1;
                    val += self.parse_term()?;
                }
                Some(b'-') => {
                    self.pos += 1;
                    val -= self.parse_term()?;
                }
                _ => break,
            }
        }
        Some(val)
    }
}

// ── Extension trait impl ──────────────────────────────────────────────────────

#[async_trait]
impl Extension for CalculatorExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    async fn initialize(&mut self) -> Result<(), ExtensionError> {
        Ok(())
    }

    async fn on_search(&self, query: &str) -> Result<Vec<ExtensionItem>, ExtensionError> {
        if query.is_empty() {
            return Ok(vec![ExtensionItem {
                title: "Calculator".to_string(),
                subtitle: Some("Type a math expression (e.g. 2 + 3 * 4)".to_string()),
                icon: Some("🧮".to_string()),
                action: "calculator-help".to_string(),
                id: None,
                detail: None,
                accessories: vec![],
                extra_actions: vec![],
                detail_metadata: vec![],
                thumbnail_rgba: None,
                grid_columns: None,
            }]);
        }

        match Self::evaluate(query) {
            Some(result) => {
                let formatted = Self::format_number(result);
                let mut items = vec![ExtensionItem {
                    title: formatted.clone(),
                    subtitle: Some(format!("= {query}")),
                    icon: Some("🧮".to_string()),
                    action: format!("calculator-result:{formatted}"),
                    id: None,
                    detail: None,
                    accessories: vec![],
                    extra_actions: vec![],
                    detail_metadata: vec![],
                    thumbnail_rgba: None,
                    grid_columns: None,
                }];
                // Secondary item: copy the expression itself
                if query != formatted {
                    items.push(ExtensionItem {
                        title: "Copy Expression".to_string(),
                        subtitle: Some(query.to_string()),
                        icon: Some("📋".to_string()),
                        action: format!("calculator-copy:{query}"),
                        id: None,
                        detail: None,
                        accessories: vec![],
                        extra_actions: vec![],
                        detail_metadata: vec![],
                        thumbnail_rgba: None,
                        grid_columns: None,
                    });
                }
                Ok(items)
            }
            None => Ok(vec![]), // Not a math expression — let other extensions handle it
        }
    }

    async fn on_action(&self, _action: &str, _item_id: Option<&str>) -> Result<(), ExtensionError> {
        Ok(())
    }

    async fn cleanup(&self) -> Result<(), ExtensionError> {
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(s: &str) -> Option<f64> {
        CalculatorExtension::evaluate(s)
    }

    #[test]
    fn basic_addition() {
        assert_eq!(eval("2+3"), Some(5.0));
    }

    #[test]
    fn basic_subtraction() {
        assert_eq!(eval("10-4"), Some(6.0));
    }

    #[test]
    fn basic_multiplication() {
        assert_eq!(eval("3*4"), Some(12.0));
    }

    #[test]
    fn basic_division() {
        assert_eq!(eval("10/4"), Some(2.5));
    }

    #[test]
    fn operator_precedence() {
        assert_eq!(eval("2+3*4"), Some(14.0));
        assert_eq!(eval("10-2*3"), Some(4.0));
    }

    #[test]
    fn parentheses_override_precedence() {
        assert_eq!(eval("(2+3)*4"), Some(20.0));
        assert_eq!(eval("(10-2)*3"), Some(24.0));
    }

    #[test]
    fn unary_minus() {
        assert_eq!(eval("-5"), Some(-5.0));
        assert_eq!(eval("10+-5"), Some(5.0));
        assert_eq!(eval("-2*-3"), Some(6.0));
    }

    #[test]
    fn decimal_numbers() {
        assert_eq!(eval("1.5+2.5"), Some(4.0));
        assert_eq!(eval("3.14*2"), Some(6.28));
    }

    #[test]
    fn whitespace_is_ignored() {
        assert_eq!(eval("2 + 3 * 4"), Some(14.0));
        assert_eq!(eval(" ( 2 + 3 ) * 4 "), Some(20.0));
    }

    #[test]
    fn invalid_expression_returns_none() {
        assert!(eval("abc").is_none());
        assert!(eval("2+").is_none());
        assert!(eval("(2+3").is_none());
        assert!(eval("").is_none());
    }

    #[test]
    fn division_by_zero_returns_none() {
        assert!(eval("5/0").is_none());
    }

    #[test]
    fn format_integer_result() {
        assert_eq!(CalculatorExtension::format_number(42.0), "42");
        assert_eq!(CalculatorExtension::format_number(-7.0), "-7");
    }

    #[test]
    fn format_decimal_result() {
        assert_eq!(CalculatorExtension::format_number(2.5), "2.5");
    }

    #[tokio::test]
    async fn on_search_empty_returns_help_item() {
        let ext = CalculatorExtension::new();
        let results = ext.on_search("").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "calculator-help");
    }

    #[tokio::test]
    async fn on_search_valid_expr_returns_result() {
        let ext = CalculatorExtension::new();
        let results = ext.on_search("2+3").await.unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].title, "5");
        assert_eq!(results[0].action, "calculator-result:5");
    }

    #[tokio::test]
    async fn on_search_invalid_expr_returns_empty() {
        let ext = CalculatorExtension::new();
        let results = ext.on_search("hello world").await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn on_search_complex_expr() {
        let ext = CalculatorExtension::new();
        let results = ext.on_search("(2+3)*4").await.unwrap();
        assert_eq!(results[0].title, "20");
    }
}
