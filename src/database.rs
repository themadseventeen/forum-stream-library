use std::env;

use sqlx::{Connection, Executor, PgConnection, Pool};

pub async fn bootstrap_tenant(db_name: String) -> anyhow::Result<Pool<sqlx::Postgres>> {
    let admin_url = env::var("PG_ADMIN_URL").expect("PG_ADMIN_URL must be set");

    let mut admin_conn: PgConnection = Connection::connect(&admin_url).await?;

    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(&db_name)
            .fetch_one(&mut admin_conn)
            .await?;

    if !exists {
        println!("Creating tenant database: {}", db_name);

        let create_sql = format!(r#"CREATE DATABASE "{}" WITH TEMPLATE template"#, db_name);

        admin_conn.execute(&*create_sql).await?;
    }

    let tenant_url = format!("{}/{}", base_url_without_db(&admin_url)?, db_name);
    let pool = Pool::connect(&tenant_url).await?;

    Ok(pool)
}

fn base_url_without_db(url: &str) -> anyhow::Result<String> {
    let idx = url
        .rfind('/')
        .ok_or_else(|| anyhow::anyhow!("Invalid PG_ADMIN_URL"))?;
    Ok(url[..idx].to_string())
}