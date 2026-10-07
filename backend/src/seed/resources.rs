//! Demo seed (slice B): Marine Science Base resources and tariffs.
//!
//! Names follow the published MSB facilities (four bedrooms, a laboratory,
//! dive and boat services from the Pitcairn community — see
//! `docs/research/marine-science-base.md`). ALL PRICES ARE FICTIONAL DEMO
//! VALUES in NZD; they are not the official MSB fee schedule.
//! Idempotent: resources are matched by name; a resource that already has a
//! tariff is left alone.

use sqlx::SqlitePool;

use crate::error::AppResult;

struct SeedResource {
    kind: &'static str,
    name: &'static str,
    description: &'static str,
    quantity: i64,
    unit_label: &'static str,
    /// Persona key of the owning provider (boat hire), if any.
    provider: Option<&'static str>,
    unit: &'static str,
    price_cents: i64,
}

const RESOURCES: &[SeedResource] = &[
    SeedResource {
        kind: "room",
        name: "MSB twin bedroom",
        description: "One of the four twin bedrooms at the Marine Science Base. Demo price, fictional.",
        quantity: 4,
        unit_label: "room",
        provider: None,
        unit: "per_night",
        price_cents: 9_500,
    },
    SeedResource {
        kind: "lab",
        name: "MSB wet laboratory",
        description: "Base laboratory: benches, balances, microscope, fridge/freezer (no −80 °C storage). Demo price, fictional.",
        quantity: 1,
        unit_label: "lab",
        provider: None,
        unit: "per_day",
        price_cents: 4_500,
    },
    SeedResource {
        kind: "equipment",
        name: "Dive compressor",
        description: "Breathing-air compressor for filling SCUBA tanks. Demo price, fictional.",
        quantity: 1,
        unit_label: "unit",
        provider: None,
        unit: "per_day",
        price_cents: 3_500,
    },
    SeedResource {
        kind: "equipment",
        name: "SCUBA tank set",
        description: "Tank, regulator and BCD set. Demo price, fictional.",
        quantity: 6,
        unit_label: "set",
        provider: None,
        unit: "per_day",
        price_cents: 1_800,
    },
    SeedResource {
        kind: "equipment",
        name: "Underwater camera housing",
        description: "Housing for photo-quadrat and transect imaging. Demo price, fictional.",
        quantity: 1,
        unit_label: "unit",
        provider: None,
        unit: "per_day",
        price_cents: 2_500,
    },
    SeedResource {
        kind: "equipment",
        name: "Survey drone (UAV)",
        description: "Aerial survey drone; use must be agreed in advance in the application. Demo price, fictional.",
        quantity: 1,
        unit_label: "unit",
        provider: None,
        unit: "per_day",
        price_cents: 6_000,
    },
    SeedResource {
        kind: "boat",
        name: "Boat charter — Bounty Bay Boat Hire (fictional)",
        description: "Skippered boat charter from a Pitcairn community provider; confirmed by the provider. Demo price, fictional.",
        quantity: 1,
        unit_label: "boat",
        provider: Some("david"),
        unit: "per_day",
        price_cents: 65_000,
    },
    SeedResource {
        kind: "service",
        name: "Field assistant",
        description: "Field/lab assistance by base staff. Demo price, fictional.",
        quantity: 1,
        unit_label: "assistant",
        provider: None,
        unit: "per_day",
        price_cents: 9_000,
    },
];

/// Tariffs of the seeded resources take effect from this date.
const TARIFF_EFFECTIVE_FROM: &str = "2024-01-01";

pub async fn seed_resources(pool: &SqlitePool) -> AppResult<()> {
    let now = crate::util::now_rfc3339();
    let persona_id = |key: &'static str| async move {
        let email = super::PERSONAS
            .iter()
            .find(|p| p.key == key)
            .map(|p| p.email)
            .unwrap_or_default();
        sqlx::query_scalar::<_, String>("SELECT id FROM users WHERE email = ?")
            .bind(email)
            .fetch_optional(pool)
            .await
    };
    let Some(admin_id) = persona_id("admin").await? else {
        return Ok(());
    };

    for r in RESOURCES {
        let provider_id = match r.provider {
            Some(key) => persona_id(key).await?,
            None => None,
        };
        let existing: Option<String> =
            sqlx::query_scalar("SELECT id FROM resources WHERE name = ?")
                .bind(r.name)
                .fetch_optional(pool)
                .await?;
        let resource_id = match existing {
            Some(id) => id,
            None => {
                let id = crate::util::new_id();
                sqlx::query(
                    "INSERT INTO resources
                     (id, kind, name, description, quantity, unit_label, provider_user_id, active, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?)",
                )
                .bind(&id)
                .bind(r.kind)
                .bind(r.name)
                .bind(r.description)
                .bind(r.quantity)
                .bind(r.unit_label)
                .bind(&provider_id)
                .bind(&now)
                .execute(pool)
                .await?;
                id
            }
        };
        let has_tariff: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM tariffs WHERE resource_id = ?")
                .bind(&resource_id)
                .fetch_one(pool)
                .await?;
        if has_tariff == 0 {
            sqlx::query(
                "INSERT INTO tariffs
                 (id, resource_id, unit, price_cents, currency, effective_from, created_by, created_at)
                 VALUES (?, ?, ?, ?, 'NZD', ?, ?, ?)",
            )
            .bind(crate::util::new_id())
            .bind(&resource_id)
            .bind(r.unit)
            .bind(r.price_cents)
            .bind(TARIFF_EFFECTIVE_FROM)
            .bind(&admin_id)
            .bind(&now)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}
