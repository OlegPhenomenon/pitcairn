use crate::error::AppResult;

/// Allocate the next reference number for `prefix` ("PIT"|"INV") and `year`,
/// formatted `PREFIX-YYYY-NNNN`. MUST be called inside a transaction (the
/// caller's `BEGIN IMMEDIATE` makes concurrent allocation safe).
pub async fn next(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    prefix: &str,
    year: i64,
) -> AppResult<String> {
    sqlx::query(
        "INSERT INTO reference_counters (prefix, year, next) VALUES (?, ?, 2)
         ON CONFLICT (prefix, year) DO UPDATE SET next = next + 1",
    )
    .bind(prefix)
    .bind(year)
    .execute(&mut **tx)
    .await?;
    let n: i64 = sqlx::query_scalar(
        "SELECT next - 1 FROM reference_counters WHERE prefix = ? AND year = ?",
    )
    .bind(prefix)
    .bind(year)
    .fetch_one(&mut **tx)
    .await?;
    Ok(format!("{prefix}-{year}-{n:04}"))
}
