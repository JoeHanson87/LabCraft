//! Recursive-descent parser for the VI DSL.

use std::collections::BTreeMap;

use crate::ast::*;
use crate::lexer::{lex, Tok, Token};
use crate::DslError;

pub fn parse(src: &str) -> Result<Program, DslError> {
    let tokens = lex(src).map_err(|e| DslError::new(e.message, e.line, e.col))?;
    let mut p = Parser { tokens, i: 0 };
    p.parse_program()
}

struct Parser {
    tokens: Vec<Token>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.tokens[self.i.min(self.tokens.len() - 1)].tok
    }
    fn peek_at(&self, n: usize) -> &Tok {
        &self.tokens[(self.i + n).min(self.tokens.len() - 1)].tok
    }
    fn pos(&self) -> (usize, usize) {
        let t = &self.tokens[self.i.min(self.tokens.len() - 1)];
        (t.line, t.col)
    }
    fn bump(&mut self) -> Tok {
        let t = self.tokens[self.i.min(self.tokens.len() - 1)].tok.clone();
        if self.i < self.tokens.len() - 1 {
            self.i += 1;
        }
        t
    }
    fn err<T>(&self, message: impl Into<String>) -> Result<T, DslError> {
        let (line, col) = self.pos();
        Err(DslError::new(message, line, col))
    }

    fn eat_ident(&mut self, name: &str) -> bool {
        if self.peek().is_ident(name) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect_ident(&mut self, what: &str) -> Result<String, DslError> {
        match self.peek().clone() {
            Tok::Ident(s) => {
                self.bump();
                Ok(s)
            }
            other => self.err(format!("expected {what}, found {other}")),
        }
    }
    fn expect_punct(&mut self, c: char) -> Result<(), DslError> {
        if self.peek().is_punct(c) {
            self.bump();
            Ok(())
        } else {
            let found = self.peek().clone();
            self.err(format!("expected `{c}`, found {found}"))
        }
    }
    fn eat_punct(&mut self, c: char) -> bool {
        if self.peek().is_punct(c) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect_str(&mut self, what: &str) -> Result<String, DslError> {
        match self.peek().clone() {
            Tok::Str(s) => {
                self.bump();
                Ok(s)
            }
            other => self.err(format!("expected {what} string, found {other}")),
        }
    }

    fn parse_program(&mut self) -> Result<Program, DslError> {
        if !self.eat_ident("vi") {
            return self.err("a VI file must start with `vi \"Name\" {`");
        }
        let name = self.expect_str("VI name")?;
        self.expect_punct('{')?;

        let mut description = String::new();
        let mut panel = Vec::new();
        let mut body = Vec::new();

        loop {
            if self.eat_punct('}') {
                break;
            }
            if matches!(self.peek(), Tok::Eof) {
                return self.err("unexpected end of file: missing closing `}` for `vi`");
            }
            if self.eat_ident("description") {
                description = self.expect_str("description")?;
                self.eat_punct(';');
                continue;
            }
            if self.eat_ident("panel") {
                if !panel.is_empty() {
                    return self.err("only one `panel` block is allowed");
                }
                panel = self.parse_panel()?;
                continue;
            }
            if self.eat_ident("diagram") {
                if !body.is_empty() {
                    return self.err("only one `diagram` block is allowed");
                }
                self.expect_punct('{')?;
                body = self.parse_stmts_until_close()?;
                continue;
            }
            return self.err(format!(
                "expected `description`, `panel` or `diagram`, found {}",
                self.peek()
            ));
        }
        if !matches!(self.peek(), Tok::Eof) {
            return self.err("unexpected input after the VI's closing `}`");
        }
        Ok(Program {
            name,
            description,
            panel,
            body,
        })
    }

    fn parse_panel(&mut self) -> Result<Vec<PanelDecl>, DslError> {
        self.expect_punct('{')?;
        let mut out = Vec::new();
        loop {
            if self.eat_punct('}') {
                break;
            }
            let (line, col) = self.pos();
            let indicator = if self.eat_ident("indicator") {
                true
            } else if self.eat_ident("control") {
                false
            } else {
                return self.err(format!(
                    "expected `control` or `indicator`, found {}",
                    self.peek()
                ));
            };
            let key = self.expect_ident("control key")?;
            let label = match self.peek().clone() {
                Tok::Str(s) => {
                    self.bump();
                    s
                }
                _ => key.clone(),
            };
            let mut pos = None;
            if self.eat_punct('@') {
                self.expect_punct('(')?;
                let x = self.expect_number()? as f32;
                self.expect_punct(',')?;
                let y = self.expect_number()? as f32;
                self.expect_punct(')')?;
                pos = Some((x, y));
            }
            let mut props = BTreeMap::new();
            if self.eat_punct('{') {
                loop {
                    if self.eat_punct('}') {
                        break;
                    }
                    let prop = self.expect_ident("property name")?;
                    self.expect_punct(':')?;
                    let value = if prop == "type" {
                        LitValue::Str(self.parse_type_name()?)
                    } else {
                        self.parse_lit_value()?
                    };
                    props.insert(prop, value);
                    if !self.eat_punct(',') && !self.peek().is_punct('}') {
                        return self.err("expected `,` or `}` after a property");
                    }
                }
            }
            out.push(PanelDecl {
                indicator,
                key,
                label,
                pos,
                props,
                line,
                col,
            });
            self.eat_punct(';');
        }
        Ok(out)
    }

    fn parse_type_name(&mut self) -> Result<String, DslError> {
        let mut s = self.expect_ident("type name")?;
        while self.peek().is_punct('[') && self.peek_at(1).is_punct(']') {
            self.bump();
            self.bump();
            s.push_str("[]");
        }
        Ok(s)
    }

    fn expect_number(&mut self) -> Result<f64, DslError> {
        let negative = self.eat_punct('-');
        match self.peek().clone() {
            Tok::Number { value, .. } => {
                self.bump();
                Ok(if negative { -value } else { value })
            }
            other => self.err(format!("expected a number, found {other}")),
        }
    }

    fn parse_lit_value(&mut self) -> Result<LitValue, DslError> {
        let negative = self.eat_punct('-');
        let value = match self.peek().clone() {
            Tok::Number { value, is_int, .. } => {
                self.bump();
                LitValue::Num {
                    value: if negative { -value } else { value },
                    is_int,
                }
            }
            Tok::Str(s) => {
                self.bump();
                LitValue::Str(s)
            }
            Tok::Ident(s) => {
                self.bump();
                match s.as_str() {
                    "true" => LitValue::Bool(true),
                    "false" => LitValue::Bool(false),
                    _ => LitValue::Ident(s),
                }
            }
            Tok::Punct('[') => {
                self.bump();
                let mut items = Vec::new();
                while !self.eat_punct(']') {
                    items.push(self.parse_lit_value()?);
                    if !self.eat_punct(',') {
                        self.expect_punct(']')?;
                        break;
                    }
                }
                LitValue::List(items)
            }
            other => return self.err(format!("expected a literal value, found {other}")),
        };
        Ok(value)
    }

    fn parse_stmts_until_close(&mut self) -> Result<Vec<Stmt>, DslError> {
        let mut out = Vec::new();
        loop {
            if self.eat_punct('}') {
                break;
            }
            if matches!(self.peek(), Tok::Eof) {
                return self.err("unexpected end of file: missing `}`");
            }
            out.push(self.parse_stmt()?);
            self.eat_punct(';');
        }
        Ok(out)
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, DslError> {
        self.expect_punct('{')?;
        self.parse_stmts_until_close()
    }

    fn parse_stmt(&mut self) -> Result<Stmt, DslError> {
        let (line, col) = self.pos();
        if self.eat_ident("let") {
            let name = self.expect_ident("binding name")?;
            self.expect_punct('=')?;
            let value = self.parse_expr()?;
            return Ok(Stmt::new(StmtKind::Let { name, value }, line, col));
        }
        if self.eat_ident("tunnel") || self.eat_ident("out") {
            let name = self.expect_ident("tunnel name")?;
            self.expect_punct('=')?;
            let value = self.parse_expr()?;
            return Ok(Stmt::new(StmtKind::Tunnel { name, value }, line, col));
        }
        if self.is_structure_start() {
            return self.parse_structure();
        }
        let expr = self.parse_expr()?;
        Ok(Stmt::new(StmtKind::Expr(expr), line, col))
    }

    fn is_structure_start(&self) -> bool {
        matches!(
            self.peek(),
            Tok::Ident(s) if matches!(s.as_str(), "while" | "for" | "case" | "event" | "sequence")
        )
    }

    fn parse_structure(&mut self) -> Result<Stmt, DslError> {
        let (line, col) = self.pos();
        let kind = if self.eat_ident("while") {
            let cond = self.parse_expr()?;
            let body = self.parse_block()?;
            StmtKind::While { cond, body }
        } else if self.eat_ident("for") {
            let var = self.expect_ident("loop variable")?;
            if !self.eat_ident("in") {
                return self.err("expected `in` after the loop variable");
            }
            let start = self.parse_expr()?;
            if !matches!(self.peek(), Tok::DotDot) {
                return self.err("expected `..` in `for i in start..end`");
            }
            self.bump();
            let end = self.parse_expr()?;
            let body = self.parse_block()?;
            StmtKind::For {
                var,
                start,
                end,
                body,
            }
        } else if self.eat_ident("case") {
            let selector = self.parse_expr()?;
            self.expect_punct('{')?;
            let mut frames = Vec::new();
            loop {
                if self.eat_punct('}') {
                    break;
                }
                if matches!(self.peek(), Tok::Eof) {
                    return self.err("unexpected end of file inside `case`");
                }
                let label = self.parse_frame_label()?;
                let body = self.parse_block()?;
                frames.push(Frame { label, body });
                self.eat_punct(',');
            }
            if frames.is_empty() {
                return self.err("a `case` needs at least one frame");
            }
            StmtKind::Case { selector, frames }
        } else if self.eat_ident("event") {
            let mut timeout = None;
            if self.eat_ident("timeout") {
                timeout = Some(self.parse_expr()?);
            }
            self.expect_punct('{')?;
            let mut frames = Vec::new();
            loop {
                if self.eat_punct('}') {
                    break;
                }
                if matches!(self.peek(), Tok::Eof) {
                    return self.err("unexpected end of file inside `event`");
                }
                let label = self.parse_frame_label()?;
                let body = self.parse_block()?;
                frames.push(Frame { label, body });
                self.eat_punct(',');
            }
            if frames.is_empty() {
                return self.err("an `event` needs at least one frame");
            }
            StmtKind::Event { timeout, frames }
        } else if self.eat_ident("sequence") {
            self.expect_punct('{')?;
            let mut frames = Vec::new();
            loop {
                if self.eat_punct('}') {
                    break;
                }
                frames.push(self.parse_block()?);
            }
            StmtKind::Sequence { frames }
        } else {
            return self.err("expected a structure keyword");
        };
        Ok(Stmt::new(kind, line, col))
    }

    fn parse_frame_label(&mut self) -> Result<String, DslError> {
        let negative = self.eat_punct('-');
        let base = match self.peek().clone() {
            Tok::Str(s) => {
                self.bump();
                LitValue::Str(s)
            }
            Tok::Number { value, is_int, .. } => {
                self.bump();
                LitValue::Num {
                    value: if negative { -value } else { value },
                    is_int,
                }
            }
            Tok::Ident(s) => {
                self.bump();
                match s.as_str() {
                    "true" => LitValue::Bool(true),
                    "false" => LitValue::Bool(false),
                    _ => LitValue::Ident(s),
                }
            }
            other => return self.err(format!("expected a frame label, found {other}")),
        };
        if matches!(self.peek(), Tok::DotDot) {
            self.bump();
            let end = match self.peek().clone() {
                Tok::Number { value, is_int, .. } => {
                    self.bump();
                    LitValue::Num { value, is_int }
                }
                Tok::Ident(s) => {
                    self.bump();
                    LitValue::Ident(s)
                }
                other => return self.err(format!("expected a range end, found {other}")),
            };
            return Ok(format!("{base}..{end}"));
        }
        Ok(match base {
            LitValue::Str(s) => s,
            LitValue::Bool(true) => "true".into(),
            LitValue::Bool(false) => "false".into(),
            other => other.to_string(),
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, DslError> {
        let start = self.parse_comparison()?;
        let mut steps = Vec::new();
        while matches!(self.peek(), Tok::Arrow) {
            self.bump();
            let name = self.expect_ident("node or indicator name after `->`")?;
            let (mut args, mut parens) = (Vec::new(), false);
            if self.eat_punct('(') {
                parens = true;
                args = self.parse_args()?;
                self.expect_punct(')')?;
            }
            steps.push(Step { name, args, parens });
        }
        if steps.is_empty() {
            Ok(start)
        } else {
            Ok(Expr::Pipe {
                start: Box::new(start),
                steps,
            })
        }
    }

    /// Comparison operators are sugar for the corresponding comparison nodes,
    /// so `temp > limit` is the same graph as `greater(temp, limit)`.
    fn parse_comparison(&mut self) -> Result<Expr, DslError> {
        let mut left = self.parse_primary()?;
        loop {
            let (node, width) = match (self.peek().clone(), self.peek_at(1).clone()) {
                (Tok::Punct('>'), Tok::Punct('=')) => ("greater_equal", 2),
                (Tok::Punct('<'), Tok::Punct('=')) => ("less_equal", 2),
                (Tok::Punct('='), Tok::Punct('=')) => ("equal", 2),
                (Tok::Punct('!'), Tok::Punct('=')) => ("not_equal", 2),
                (Tok::Punct('>'), _) => ("greater", 1),
                (Tok::Punct('<'), _) => ("less", 1),
                _ => break,
            };
            for _ in 0..width {
                self.bump();
            }
            let right = self.parse_primary()?;
            left = Expr::Call {
                name: node.to_string(),
                args: vec![
                    Arg {
                        name: None,
                        value: left,
                    },
                    Arg {
                        name: None,
                        value: right,
                    },
                ],
            };
        }
        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<Expr, DslError> {
        let negative = self.eat_punct('-');
        let mut expr = match self.peek().clone() {
            Tok::Number { value, is_int, .. } => {
                self.bump();
                Expr::Lit(LitValue::Num {
                    value: if negative { -value } else { value },
                    is_int,
                })
            }
            Tok::Str(s) => {
                self.bump();
                Expr::Lit(LitValue::Str(s))
            }
            Tok::Ident(s) => {
                if self.is_structure_start() {
                    return self.parse_structure().map(|s| Expr::Structure(Box::new(s)));
                }
                self.bump();
                match s.as_str() {
                    "true" => Expr::Lit(LitValue::Bool(true)),
                    "false" => Expr::Lit(LitValue::Bool(false)),
                    _ => {
                        if self.eat_punct('(') {
                            let args = self.parse_args()?;
                            self.expect_punct(')')?;
                            Expr::Call { name: s, args }
                        } else {
                            Expr::Ident(s)
                        }
                    }
                }
            }
            Tok::Punct('(') => {
                self.bump();
                let inner = self.parse_expr()?;
                self.expect_punct(')')?;
                inner
            }
            Tok::Punct('[') => {
                self.bump();
                let mut items = Vec::new();
                while !self.eat_punct(']') {
                    items.push(self.parse_expr()?);
                    if !self.eat_punct(',') {
                        self.expect_punct(']')?;
                        break;
                    }
                }
                Expr::Array(items)
            }
            Tok::Punct('{') => {
                self.bump();
                let mut fields = Vec::new();
                while !self.eat_punct('}') {
                    let key = match self.peek().clone() {
                        Tok::Ident(s) => {
                            self.bump();
                            s
                        }
                        Tok::Str(s) => {
                            self.bump();
                            s
                        }
                        other => return self.err(format!("expected a field name, found {other}")),
                    };
                    self.expect_punct(':')?;
                    let value = self.parse_expr()?;
                    fields.push((key, value));
                    if !self.eat_punct(',') && !self.peek().is_punct('}') {
                        return self.err("expected `,` or `}` in a cluster literal");
                    }
                }
                Expr::Cluster(fields)
            }
            other => return self.err(format!("expected an expression, found {other}")),
        };
        while self.eat_punct('.') {
            let name = self.expect_ident("field or output port name")?;
            expr = Expr::Field {
                base: Box::new(expr),
                name,
            };
        }
        Ok(expr)
    }

    fn parse_args(&mut self) -> Result<Vec<Arg>, DslError> {
        let mut out = Vec::new();
        if self.peek().is_punct(')') {
            return Ok(out);
        }
        loop {
            let named = matches!(self.peek(), Tok::Ident(_)) && self.peek_at(1).is_punct(':');
            if named {
                let name = self.expect_ident("argument name")?;
                self.expect_punct(':')?;
                let value = self.parse_expr()?;
                out.push(Arg {
                    name: Some(name),
                    value,
                });
            } else {
                let value = self.parse_expr()?;
                out.push(Arg { name: None, value });
            }
            if self.eat_punct(',') {
                if self.peek().is_punct(')') {
                    break;
                }
                continue;
            }
            break;
        }
        Ok(out)
    }
}
