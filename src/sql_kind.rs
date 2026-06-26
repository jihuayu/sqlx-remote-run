use sqlparser::ast::Statement;
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Token, Tokenizer};

use crate::RemoteSqlError;

/// Broad statement category used by permission checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatementKind {
    /// Read-only statements that return data.
    Read,
    /// Data manipulation statements.
    Write,
    /// Administrative statements such as DDL, PRAGMA, VACUUM, and ANALYZE.
    Admin,
}

/// Parses and classifies one SQLite SQL statement.
///
/// # Errors
///
/// Returns [`RemoteSqlError`] when the SQL is empty, invalid, contains more
/// than one statement, or maps to an unsupported statement kind.
pub fn classify_sql(sql: &str) -> Result<StatementKind, RemoteSqlError> {
    if sql.trim().is_empty() {
        return Err(RemoteSqlError::EmptySql);
    }

    let dialect = SQLiteDialect {};
    let statements = match Parser::parse_sql(&dialect, sql) {
        Ok(statements) => statements,
        Err(err) => {
            if first_keyword(sql)?.as_str() == "REINDEX" {
                ensure_single_tokenized_statement(sql)?;
                return Ok(StatementKind::Admin);
            }
            return Err(RemoteSqlError::InvalidSql(err.to_string()));
        }
    };

    let [statement] = statements.as_slice() else {
        return Err(RemoteSqlError::ExpectedSingleStatement);
    };

    classify_statement(sql, statement)
}

fn classify_statement(sql: &str, statement: &Statement) -> Result<StatementKind, RemoteSqlError> {
    match statement {
        Statement::Query(_) => Ok(StatementKind::Read),
        Statement::Insert(_) | Statement::Update(_) | Statement::Delete(_) => {
            Ok(StatementKind::Write)
        }
        _ => classify_by_first_keyword(sql),
    }
}

fn classify_by_first_keyword(sql: &str) -> Result<StatementKind, RemoteSqlError> {
    let keyword = first_keyword(sql)?;
    match keyword.as_str() {
        "SELECT" | "VALUES" => Ok(StatementKind::Read),
        "INSERT" | "UPDATE" | "DELETE" | "REPLACE" => Ok(StatementKind::Write),
        "CREATE" | "ALTER" | "DROP" | "PRAGMA" | "VACUUM" | "ANALYZE" | "REINDEX" => {
            Ok(StatementKind::Admin)
        }
        _ => Err(RemoteSqlError::UnsupportedStatement),
    }
}

fn first_keyword(sql: &str) -> Result<String, RemoteSqlError> {
    let tokens = tokenize(sql)?;

    for token in tokens {
        match token {
            Token::Whitespace(_) => {}
            Token::Word(word) if word.quote_style.is_none() => {
                return Ok(word.value.to_ascii_uppercase());
            }
            _ => return Err(RemoteSqlError::UnsupportedStatement),
        }
    }

    Err(RemoteSqlError::EmptySql)
}

fn ensure_single_tokenized_statement(sql: &str) -> Result<(), RemoteSqlError> {
    let tokens = tokenize(sql)?;
    let mut saw_statement_token = false;
    let mut saw_terminator = false;

    for token in tokens {
        match token {
            Token::Whitespace(_) => {}
            Token::SemiColon => {
                if !saw_statement_token || saw_terminator {
                    return Err(RemoteSqlError::ExpectedSingleStatement);
                }
                saw_terminator = true;
            }
            _ if saw_terminator => return Err(RemoteSqlError::ExpectedSingleStatement),
            _ => saw_statement_token = true,
        }
    }

    if saw_statement_token {
        Ok(())
    } else {
        Err(RemoteSqlError::EmptySql)
    }
}

fn tokenize(sql: &str) -> Result<Vec<Token>, RemoteSqlError> {
    let dialect = SQLiteDialect {};
    Tokenizer::new(&dialect, sql)
        .tokenize()
        .map_err(|err| RemoteSqlError::InvalidSql(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_sql_returns_read_for_select_with_leading_comment() {
        let kind = classify_sql("-- comment\nselect 1").unwrap();

        assert_eq!(kind, StatementKind::Read);
    }

    #[test]
    fn classify_sql_returns_write_for_insert() {
        let kind = classify_sql("insert into users(id) values (?)").unwrap();

        assert_eq!(kind, StatementKind::Write);
    }

    #[test]
    fn classify_sql_returns_admin_for_create_table() {
        let kind = classify_sql("create table users(id integer primary key)").unwrap();

        assert_eq!(kind, StatementKind::Admin);
    }

    #[test]
    fn classify_sql_returns_admin_for_sqlite_admin_statements() {
        for sql in ["pragma user_version", "vacuum", "analyze", "reindex"] {
            let kind = classify_sql(sql).unwrap();

            assert_eq!(kind, StatementKind::Admin);
        }
    }

    #[test]
    fn classify_sql_rejects_multiple_statements() {
        let error = classify_sql("select 1; select 2").unwrap_err();

        assert!(matches!(error, RemoteSqlError::ExpectedSingleStatement));
    }

    #[test]
    fn classify_sql_rejects_unknown_statement() {
        let error = classify_sql("explain select 1").unwrap_err();

        assert!(matches!(error, RemoteSqlError::UnsupportedStatement));
    }
}
