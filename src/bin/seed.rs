//! Idempotent seeder for the e-commerce test dataset described in
//! `docs/e_commerce_tables_design.md`. The binary owns the `public` and
//! `audit` schemas plus one view in `public`; it drops everything it owns,
//! recreates the objects, then inserts interconnected rows. The dataset is
//! deterministic given `SEED_RNG`, which makes golden-file tests reproducible.
//!
//! Required env: `DATABASE_URL`.
//! Optional env: `SEED_RESET` (`skip` to leave existing tables), `SEED_RNG`,
//! `SEED_USERS`, `SEED_PRODUCTS`, `SEED_ORDERS`, `SEED_REVIEWS`, `SEED_EVENTS`.
//!
//! Run with: `cargo run --bin seed`.

use std::collections::{HashMap, HashSet};
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

// =====================================================================
// Reference data
// =====================================================================

const FIRST_NAMES: &[&str] = &[
    "alex", "jamie", "morgan", "taylor", "jordan", "casey", "riley", "sam", "drew", "elena",
    "maya", "kai", "wei", "lin", "ari", "noah",
];

const LAST_NAMES: &[&str] = &[
    "li", "nguyen", "garcia", "smith", "khan", "ito", "ng", "park", "wolf", "reyes", "tanaka",
    "muller", "brown", "ozawa", "rivera", "fischer",
];

const EMAIL_DOMAINS: &[&str] = &["example.com", "mail.test", "corp.io", "shop.dev", "acme.co"];

const CITIES: &[&str] = &[
    "Berlin",
    "Singapore",
    "Toronto",
    "Lisbon",
    "Kyoto",
    "Austin",
    "Mumbai",
    "Seoul",
    "Buenos Aires",
    "Stockholm",
    "Cape Town",
    "Helsinki",
];

const REGIONS: &[&str] = &["CA", "NY", "BE", "SG", "ON", "JP", "TX", "BR", "SE", "ZA"];

const COUNTRIES: &[&str] = &["US", "CA", "DE", "SG", "JP", "BR", "SE", "ZA", "FI"];

const STREET_NAMES: &[&str] = &[
    "Main", "Oak", "Pine", "Cedar", "Maple", "Elm", "Park", "View", "Lake", "Hill",
];

const TOP_CATEGORIES: &[&str] = &[
    "Books",
    "Electronics",
    "Home & Kitchen",
    "Apparel",
    "Toys & Games",
];

const SUB_CATEGORIES: &[(&str, &str)] = &[
    ("Books", "Fiction"),
    ("Books", "Non-fiction"),
    ("Books", "Children"),
    ("Electronics", "Audio"),
    ("Electronics", "Computers"),
    ("Electronics", "Smartphones"),
    ("Home & Kitchen", "Cookware"),
    ("Home & Kitchen", "Small Appliances"),
    ("Apparel", "Menswear"),
    ("Apparel", "Womenswear"),
    ("Toys & Games", "Board Games"),
];

const PRODUCT_KINDS: &[&str] = &[
    "Widget", "Gadget", "Thing", "Item", "Piece", "Tool", "Kit", "Bundle",
];

const PRODUCT_COLORS: &[&str] = &[
    "Red", "Blue", "Green", "Black", "White", "Silver", "Gold", "Wood",
];

const PRODUCT_SIZES: &[&str] = &["Mini", "Standard", "Pro", "XL", "Compact"];

const WAREHOUSES: &[&str] = &["main", "east", "west"];

const CARRIERS: &[&str] = &["ups", "fedex", "dhl", "internal"];

const ORDER_STATUSES: &[&str] = &[
    "pending",
    "paid",
    "shipped",
    "delivered",
    "cancelled",
    "refunded",
];
const ORDER_STATUS_WEIGHTS: &[u32] = &[10, 40, 25, 15, 5, 5];

const PAYMENT_METHODS: &[&str] = &["card", "paypal", "bank_transfer", "cod"];

const AUDIT_ACTIONS: &[&str] = &[
    "login", "logout", "create", "update", "delete", "view", "export",
];

const AUDIT_ENTITIES: &[&str] = &["user", "order", "product", "category", "review", "payment"];

const REVIEW_TITLES: &[&str] = &[
    "Nice",
    "Solid",
    "Disappointed",
    "Exactly as described",
    "Would buy again",
    "Mixed feelings",
    "Best in class",
];

// =====================================================================
// DDL: drop everything we own; recreate from scratch.
// =====================================================================

const DROP_ALL: &[&str] = &[
    "DROP TABLE IF EXISTS public.reviews       CASCADE",
    "DROP TABLE IF EXISTS public.shipments     CASCADE",
    "DROP TABLE IF EXISTS public.payments      CASCADE",
    "DROP TABLE IF EXISTS public.order_items   CASCADE",
    "DROP TABLE IF EXISTS public.orders        CASCADE",
    "DROP TABLE IF EXISTS public.inventory     CASCADE",
    "DROP TABLE IF EXISTS public.products      CASCADE",
    "DROP TABLE IF EXISTS public.categories    CASCADE",
    "DROP TABLE IF EXISTS public.addresses     CASCADE",
    "DROP TABLE IF EXISTS public.users         CASCADE",
    "DROP VIEW  IF EXISTS public.user_order_summary",
    "DROP TABLE IF EXISTS audit.event_log      CASCADE",
    "DROP SCHEMA IF EXISTS audit CASCADE",
    "DROP SCHEMA IF EXISTS public CASCADE",
];

const CREATE_ALL: &[&str] = &[
    r#"CREATE TABLE public.users (
         id          BIGSERIAL PRIMARY KEY,
         email       TEXT UNIQUE NOT NULL,
         full_name   TEXT NOT NULL,
         phone       TEXT,
         status      TEXT NOT NULL DEFAULT 'active',
         created_at  BIGINT NOT NULL
       )"#,
    "CREATE INDEX idx_users_status ON public.users(status)",
    r#"CREATE TABLE public.addresses (
         id           BIGSERIAL PRIMARY KEY,
         user_id      BIGINT NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
         line1        TEXT NOT NULL,
         line2        TEXT,
         city         TEXT NOT NULL,
         region       TEXT NOT NULL,
         country      TEXT NOT NULL,
         postal_code  TEXT NOT NULL,
         is_default   BOOLEAN NOT NULL DEFAULT FALSE
       )"#,
    "CREATE INDEX idx_addresses_user ON public.addresses(user_id)",
    r#"CREATE TABLE public.categories (
         id          BIGSERIAL PRIMARY KEY,
         parent_id   BIGINT REFERENCES public.categories(id) ON DELETE SET NULL,
         name        TEXT NOT NULL,
         slug        TEXT UNIQUE NOT NULL,
         description TEXT
       )"#,
    "CREATE INDEX idx_categories_parent ON public.categories(parent_id)",
    r#"CREATE TABLE public.products (
         id          BIGSERIAL PRIMARY KEY,
         category_id BIGINT NOT NULL REFERENCES public.categories(id) ON DELETE RESTRICT,
         sku         TEXT UNIQUE NOT NULL,
         name        TEXT NOT NULL,
         description TEXT,
         price_cents BIGINT NOT NULL CHECK (price_cents >= 0),
         currency    TEXT NOT NULL DEFAULT 'USD',
         status      TEXT NOT NULL DEFAULT 'on_sale',
         created_at  BIGINT NOT NULL
       )"#,
    "CREATE INDEX idx_products_category ON public.products(category_id)",
    "CREATE INDEX idx_products_status   ON public.products(status)",
    r#"CREATE TABLE public.inventory (
         product_id  BIGINT PRIMARY KEY REFERENCES public.products(id) ON DELETE CASCADE,
         warehouse   TEXT NOT NULL DEFAULT 'main',
         on_hand     BIGINT NOT NULL DEFAULT 0 CHECK (on_hand >= 0),
         reserved    BIGINT NOT NULL DEFAULT 0 CHECK (reserved >= 0),
         updated_at  BIGINT NOT NULL
       )"#,
    r#"CREATE TABLE public.orders (
         id              BIGSERIAL PRIMARY KEY,
         user_id         BIGINT NOT NULL REFERENCES public.users(id) ON DELETE RESTRICT,
         ship_address_id BIGINT NOT NULL REFERENCES public.addresses(id) ON DELETE RESTRICT,
         status          TEXT NOT NULL DEFAULT 'pending',
         subtotal_cents  BIGINT NOT NULL,
         shipping_cents  BIGINT NOT NULL DEFAULT 0,
         total_cents     BIGINT NOT NULL,
         placed_at       BIGINT NOT NULL
       )"#,
    "CREATE INDEX idx_orders_user   ON public.orders(user_id)",
    "CREATE INDEX idx_orders_status ON public.orders(status)",
    r#"CREATE TABLE public.order_items (
         id               BIGSERIAL PRIMARY KEY,
         order_id         BIGINT NOT NULL REFERENCES public.orders(id) ON DELETE CASCADE,
         product_id       BIGINT NOT NULL REFERENCES public.products(id) ON DELETE RESTRICT,
         quantity         INTEGER NOT NULL CHECK (quantity > 0),
         unit_price_cents BIGINT NOT NULL CHECK (unit_price_cents >= 0),
         line_total_cents BIGINT NOT NULL CHECK (line_total_cents >= 0)
       )"#,
    "CREATE INDEX idx_order_items_order   ON public.order_items(order_id)",
    "CREATE INDEX idx_order_items_product ON public.order_items(product_id)",
    r#"CREATE TABLE public.payments (
         id           BIGSERIAL PRIMARY KEY,
         order_id     BIGINT UNIQUE NOT NULL REFERENCES public.orders(id) ON DELETE CASCADE,
         method       TEXT NOT NULL,
         amount_cents BIGINT NOT NULL,
         status       TEXT NOT NULL,
         provider_ref TEXT,
         captured_at  BIGINT
       )"#,
    r#"CREATE TABLE public.shipments (
         id              BIGSERIAL PRIMARY KEY,
         order_id        BIGINT NOT NULL REFERENCES public.orders(id) ON DELETE CASCADE,
         ship_address_id BIGINT NOT NULL REFERENCES public.addresses(id) ON DELETE RESTRICT,
         carrier         TEXT NOT NULL,
         tracking_no     TEXT,
         status          TEXT NOT NULL,
         shipped_at      BIGINT,
         delivered_at    BIGINT
       )"#,
    "CREATE INDEX idx_shipments_order   ON public.shipments(order_id)",
    "CREATE INDEX idx_shipments_status ON public.shipments(status)",
    r#"CREATE TABLE public.reviews (
         id         BIGSERIAL PRIMARY KEY,
         user_id    BIGINT NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
         product_id BIGINT NOT NULL REFERENCES public.products(id) ON DELETE CASCADE,
         rating     SMALLINT NOT NULL CHECK (rating BETWEEN 1 AND 5),
         title      TEXT,
         body       TEXT,
         created_at BIGINT NOT NULL,
         UNIQUE (user_id, product_id)
       )"#,
    "CREATE INDEX idx_reviews_product ON public.reviews(product_id)",
    r#"CREATE TABLE audit.event_log (
         id         BIGSERIAL PRIMARY KEY,
         actor      TEXT NOT NULL,
         action     TEXT NOT NULL,
         entity     TEXT NOT NULL,
         entity_id  BIGINT,
         created_at BIGINT NOT NULL
       )"#,
    "CREATE INDEX idx_event_log_actor ON audit.event_log(actor)",
];

const USER_ORDER_SUMMARY_VIEW: &str = r#"
CREATE VIEW public.user_order_summary AS
SELECT u.id                              AS user_id,
       u.email,
       u.full_name,
       COUNT(o.id)                       AS order_count,
       COALESCE(SUM(o.total_cents), 0)   AS lifetime_cents
FROM public.users u
LEFT JOIN public.orders o ON o.user_id = u.id
GROUP BY u.id, u.email, u.full_name
"#;

// =====================================================================
// Small helpers
// =====================================================================

fn env_u64(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn reset_disabled() -> bool {
    matches!(
        env::var("SEED_RESET").as_deref(),
        Ok("skip") | Ok("false") | Ok("0")
    )
}

fn one_of<'a, R: Rng>(rng: &mut R, xs: &'a [&'a str]) -> &'a str {
    xs[rng.gen_range(0..xs.len())]
}

fn pick_weighted<'a, R: Rng>(rng: &mut R, choices: &'a [&'a str], weights: &[u32]) -> &'a str {
    let total: u32 = weights.iter().sum();
    let mut roll: u32 = rng.gen_range(0..total);
    for (c, w) in choices.iter().copied().zip(weights.iter()) {
        if roll < *w {
            return c;
        }
        roll -= w;
    }
    choices[choices.len() - 1]
}

fn rand_email<R: Rng>(rng: &mut R, idx: usize) -> String {
    let first = one_of(rng, FIRST_NAMES);
    let last = one_of(rng, LAST_NAMES);
    let domain = one_of(rng, EMAIL_DOMAINS);
    format!("{first}.{last}.{idx}@{domain}")
}

fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_dash = true;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

async fn reset(pool: &PgPool) -> sqlx::Result<()> {
    for stmt in DROP_ALL {
        sqlx::raw_sql(sqlx::AssertSqlSafe(*stmt))
            .execute(pool)
            .await?;
    }
    Ok(())
}

async fn bootstrap(pool: &PgPool) -> sqlx::Result<()> {
    sqlx::query("CREATE SCHEMA public").execute(pool).await?;
    sqlx::query("CREATE SCHEMA audit").execute(pool).await?;
    for stmt in CREATE_ALL {
        sqlx::raw_sql(sqlx::AssertSqlSafe(*stmt))
            .execute(pool)
            .await?;
    }
    Ok(())
}

// =====================================================================
// Seeders
// =====================================================================

async fn seed_users(pool: &PgPool, n: u64, rng: &mut StdRng, now: i64) -> sqlx::Result<Vec<i64>> {
    let mut ids = Vec::with_capacity(n as usize);
    for i in 0..n {
        let email = rand_email(rng, i as usize);
        let first = one_of(rng, FIRST_NAMES);
        let last = one_of(rng, LAST_NAMES);
        let full_name = format!("{first} {last}");
        let phone: Option<String> = if rng.gen_bool(0.7) {
            let digits: u32 = rng.gen_range(100_000_000..1_000_000_000);
            Some(format!("+1{digits:010}"))
        } else {
            None
        };
        let status = if rng.gen_bool(0.92) {
            "active"
        } else {
            "disabled"
        };
        let created_at = now - rng.gen_range(0..(60 * 60 * 24 * 365_i64));
        let id: i64 = sqlx::query_scalar::<_, i64>(
            "INSERT INTO public.users(email, full_name, phone, status, created_at)
             VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(&email)
        .bind(&full_name)
        .bind(&phone)
        .bind(status)
        .bind(created_at)
        .fetch_one(pool)
        .await?;
        ids.push(id);
    }
    Ok(ids)
}

async fn seed_addresses(
    pool: &PgPool,
    user_ids: &[i64],
    rng: &mut StdRng,
) -> sqlx::Result<HashMap<i64, Vec<i64>>> {
    let mut map: HashMap<i64, Vec<i64>> = HashMap::with_capacity(user_ids.len());
    for &uid in user_ids {
        let count = rng.gen_range(1..=3_u32);
        let mut inserted_for_user = Vec::with_capacity(count as usize);
        for i in 0..count {
            let line1 = format!(
                "{} {} {}",
                rng.gen_range(1..9999_u32),
                one_of(rng, STREET_NAMES),
                rng.gen_range(if i == 0 { 1..3 } else { 1..5 }),
            );
            let line2: Option<String> = if i == 0 && rng.gen_bool(0.3) {
                let apt: u32 = rng.gen_range(1..999);
                Some(format!("Apt {apt}"))
            } else {
                None
            };
            let city = one_of(rng, CITIES).to_string();
            let region = one_of(rng, REGIONS).to_string();
            let country = one_of(rng, COUNTRIES).to_string();
            let postal = format!("{:05}", rng.gen_range(0..100_000_u32));
            let is_default = i == 0;
            let id: i64 = sqlx::query_scalar::<_, i64>(
                "INSERT INTO public.addresses(user_id, line1, line2, city, region,
                                              country, postal_code, is_default)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
            )
            .bind(uid)
            .bind(&line1)
            .bind(&line2)
            .bind(&city)
            .bind(&region)
            .bind(&country)
            .bind(&postal)
            .bind(is_default)
            .fetch_one(pool)
            .await?;
            inserted_for_user.push(id);
        }
        map.insert(uid, inserted_for_user);
    }
    Ok(map)
}

async fn seed_categories(pool: &PgPool) -> sqlx::Result<Vec<i64>> {
    let mut tops = Vec::with_capacity(TOP_CATEGORIES.len());
    for name in TOP_CATEGORIES {
        let slug = slugify(name);
        let id: i64 = sqlx::query_scalar::<_, i64>(
            "INSERT INTO public.categories(parent_id, name, slug, description)
             VALUES (NULL, $1, $2, $3) RETURNING id",
        )
        .bind(*name)
        .bind(&slug)
        .bind(Some(format!("Top-level category: {name}")))
        .fetch_one(pool)
        .await?;
        tops.push(id);
    }
    let mut all = tops.clone();
    for (parent_name, name) in SUB_CATEGORIES {
        let idx = TOP_CATEGORIES
            .iter()
            .position(|n| *n == *parent_name)
            .expect("parent category must exist in TOP_CATEGORIES");
        let parent_id = tops[idx];
        let combined = format!("{parent_name} {name}");
        let slug = slugify(&combined);
        let id: i64 = sqlx::query_scalar::<_, i64>(
            "INSERT INTO public.categories(parent_id, name, slug, description)
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(parent_id)
        .bind(*name)
        .bind(&slug)
        .bind(Some(format!("Sub-category under {parent_name}")))
        .fetch_one(pool)
        .await?;
        all.push(id);
    }
    Ok(all)
}

async fn seed_products(
    pool: &PgPool,
    category_ids: &[i64],
    n: u64,
    rng: &mut StdRng,
    now: i64,
) -> sqlx::Result<HashMap<i64, i64>> {
    let mut prices: HashMap<i64, i64> = HashMap::with_capacity(n as usize);
    for i in 0..n {
        let cat = category_ids[rng.gen_range(0..category_ids.len())];
        let sku = format!("SKU-{:06}", i);
        let kind = one_of(rng, PRODUCT_KINDS);
        let color = one_of(rng, PRODUCT_COLORS);
        let size = one_of(rng, PRODUCT_SIZES);
        let name = format!("{color} {size} {kind}");
        let desc: Option<String> = if rng.gen_bool(0.7) {
            Some(format!("A high-quality {kind} for everyday use."))
        } else {
            None
        };
        let price_cents: i64 = rng.gen_range(199..39_999);
        let currency = "USD";
        let status = if rng.gen_bool(0.85) {
            "on_sale"
        } else if rng.gen_bool(0.7) {
            "out_of_stock"
        } else {
            "discontinued"
        };
        let created_at = now - rng.gen_range(0..(60 * 60 * 24 * 365_i64));
        let id: i64 = sqlx::query_scalar::<_, i64>(
            "INSERT INTO public.products(category_id, sku, name, description,
                                         price_cents, currency, status, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
        )
        .bind(cat)
        .bind(&sku)
        .bind(&name)
        .bind(&desc)
        .bind(price_cents)
        .bind(currency)
        .bind(status)
        .bind(created_at)
        .fetch_one(pool)
        .await?;
        prices.insert(id, price_cents);
    }
    Ok(prices)
}

async fn seed_inventory(
    pool: &PgPool,
    product_ids: &[i64],
    rng: &mut StdRng,
    now: i64,
) -> sqlx::Result<()> {
    for &pid in product_ids {
        let wh = one_of(rng, WAREHOUSES);
        let on_hand: i64 = rng.gen_range(0..500);
        // Range must be non-empty for `gen_range`. When `on_hand == 0`
        // (nothing in stock) we fall back to an upper of 1 so the call
        // stays well-formed; `gen_range(0..1)` then yields 0, which is
        // the only valid value for `reserved` at zero stock.
        let upper = on_hand.min(50).max(1);
        let reserved: i64 = rng.gen_range(0..upper);
        sqlx::query(
            "INSERT INTO public.inventory(product_id, warehouse, on_hand, reserved, updated_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(pid)
        .bind(wh)
        .bind(on_hand)
        .bind(reserved)
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn seed_orders(
    pool: &PgPool,
    user_ids: &[i64],
    addresses_by_user: &HashMap<i64, Vec<i64>>,
    n: u64,
    rng: &mut StdRng,
    now: i64,
) -> sqlx::Result<Vec<i64>> {
    let mut ids = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let uid = user_ids[rng.gen_range(0..user_ids.len())];
        let addrs = addresses_by_user
            .get(&uid)
            .expect("every user has at least one address");
        let ship_address_id = addrs[rng.gen_range(0..addrs.len())];
        let status = pick_weighted(rng, ORDER_STATUSES, ORDER_STATUS_WEIGHTS).to_string();
        let subtotal_cents = 0_i64;
        let shipping_cents: i64 = rng.gen_range(0..2_500);
        let total_cents = subtotal_cents + shipping_cents;
        let placed_at = now - rng.gen_range(0..(60 * 60 * 24 * 180_i64));
        let id: i64 = sqlx::query_scalar::<_, i64>(
            "INSERT INTO public.orders(user_id, ship_address_id, status,
                                       subtotal_cents, shipping_cents, total_cents, placed_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        )
        .bind(uid)
        .bind(ship_address_id)
        .bind(&status)
        .bind(subtotal_cents)
        .bind(shipping_cents)
        .bind(total_cents)
        .bind(placed_at)
        .fetch_one(pool)
        .await?;
        ids.push(id);
    }
    Ok(ids)
}

async fn seed_order_items(
    pool: &PgPool,
    order_ids: &[i64],
    product_prices: &HashMap<i64, i64>,
    rng: &mut StdRng,
) -> sqlx::Result<()> {
    let product_id_list: Vec<i64> = product_prices.keys().copied().collect();
    for &oid in order_ids {
        let item_count = rng.gen_range(1..=5_u32);
        let mut picked: HashSet<i64> = HashSet::with_capacity(item_count as usize);
        for _ in 0..item_count {
            let pid = if product_id_list.len() > picked.len() {
                loop {
                    let candidate = product_id_list[rng.gen_range(0..product_id_list.len())];
                    if picked.insert(candidate) {
                        break candidate;
                    }
                }
            } else {
                product_id_list[rng.gen_range(0..product_id_list.len())]
            };
            let quantity: i32 = rng.gen_range(1..5);
            let unit_price = product_prices[&pid];
            let line_total = unit_price * quantity as i64;
            sqlx::query(
                "INSERT INTO public.order_items(order_id, product_id, quantity,
                                                unit_price_cents, line_total_cents)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(oid)
            .bind(pid)
            .bind(quantity)
            .bind(unit_price)
            .bind(line_total)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

async fn backfill_order_totals(pool: &PgPool) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE public.orders o
         SET subtotal_cents = COALESCE(s.sum_cents, 0)
         FROM (SELECT order_id, SUM(line_total_cents) AS sum_cents
               FROM public.order_items
               GROUP BY order_id) AS s
         WHERE s.order_id = o.id",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE public.orders
         SET total_cents = subtotal_cents + shipping_cents",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_payments(pool: &PgPool, rng: &mut StdRng, now: i64) -> sqlx::Result<()> {
    let rows = sqlx::query("SELECT id, total_cents, status FROM public.orders")
        .fetch_all(pool)
        .await?;
    for row in rows {
        let id: i64 = row.get("id");
        let total_cents: i64 = row.get("total_cents");
        let status: String = row.get("status");
        let (payment_status, captured_at): (&str, Option<i64>) = match status.as_str() {
            "pending" => (
                if rng.gen_bool(0.5) {
                    "authorized"
                } else {
                    "pending"
                },
                None,
            ),
            "paid" | "shipped" | "delivered" => (
                "captured",
                Some(now - rng.gen_range(0..(60 * 60 * 24 * 30_i64))),
            ),
            "refunded" => (
                "refunded",
                Some(now - rng.gen_range(0..(60 * 60 * 24 * 30_i64))),
            ),
            "cancelled" => ("failed", None),
            _ => ("pending", None),
        };
        let method = one_of(rng, PAYMENT_METHODS);
        let provider_ref: Option<String> = match method {
            "card" => Some(format!("ch_{:016x}", rng.r#gen::<u64>())),
            "paypal" => Some(format!(
                "PAYID-{:012}",
                rng.r#gen::<u64>() & 0x0000_ffff_ffff_ffff_u64
            )),
            _ => None,
        };
        sqlx::query(
            "INSERT INTO public.payments(order_id, method, amount_cents, status,
                                          provider_ref, captured_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id)
        .bind(method)
        .bind(total_cents)
        .bind(payment_status)
        .bind(&provider_ref)
        .bind(captured_at)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn seed_shipments(
    pool: &PgPool,
    addresses_by_user: &HashMap<i64, Vec<i64>>,
    rng: &mut StdRng,
) -> sqlx::Result<()> {
    let rows = sqlx::query("SELECT id, user_id, status, placed_at FROM public.orders")
        .fetch_all(pool)
        .await?;
    for row in rows {
        let id: i64 = row.get("id");
        let user_id: i64 = row.get("user_id");
        let status: String = row.get("status");
        let placed_at: i64 = row.get("placed_at");
        let (ship_status, shipped_at, delivered_at): (&str, Option<i64>, Option<i64>) =
            match status.as_str() {
                "pending" | "cancelled" => continue,
                "paid" => ("pending", None, None),
                "shipped" => (
                    "in_transit",
                    Some(placed_at + rng.gen_range(3_600_i64..(86_400 * 2))),
                    None,
                ),
                "delivered" => (
                    "delivered",
                    Some(placed_at + rng.gen_range(3_600_i64..(86_400 * 2))),
                    Some(placed_at + rng.gen_range((86_400 * 2)..(86_400 * 5))),
                ),
                "refunded" => (
                    "returned",
                    Some(placed_at + rng.gen_range(3_600_i64..(86_400 * 2))),
                    Some(placed_at + rng.gen_range((86_400 * 2)..(86_400 * 5))),
                ),
                _ => continue,
            };
        let addrs = addresses_by_user
            .get(&user_id)
            .expect("every user has at least one address");
        let ship_address_id = addrs[rng.gen_range(0..addrs.len())];
        let carrier = one_of(rng, CARRIERS);
        let tracking_no = format!("{}{:010}", carrier.to_ascii_uppercase(), rng.r#gen::<u32>());
        sqlx::query(
            "INSERT INTO public.shipments(order_id, ship_address_id, carrier,
                                          tracking_no, status, shipped_at, delivered_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(id)
        .bind(ship_address_id)
        .bind(carrier)
        .bind(&tracking_no)
        .bind(ship_status)
        .bind(shipped_at)
        .bind(delivered_at)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn seed_reviews(
    pool: &PgPool,
    user_ids: &[i64],
    product_prices: &HashMap<i64, i64>,
    n: u64,
    rng: &mut StdRng,
    now: i64,
) -> sqlx::Result<()> {
    let product_ids: Vec<i64> = product_prices.keys().copied().collect();
    let max_attempts = n.saturating_mul(4).max(1);
    let mut inserted = 0_u64;
    let mut attempts = 0_u64;
    while inserted < n && attempts < max_attempts {
        attempts += 1;
        let uid = user_ids[rng.gen_range(0..user_ids.len())];
        let pid = product_ids[rng.gen_range(0..product_ids.len())];
        let rating: i16 = match rng.gen_range(0..10_u32) {
            0..=1 => 1,
            2..=3 => 2,
            4..=5 => 3,
            6..=7 => 4,
            _ => 5,
        };
        let title: Option<String> = if rng.gen_bool(0.5) {
            Some(one_of(rng, REVIEW_TITLES).to_string())
        } else {
            None
        };
        let body: Option<String> = if rng.gen_bool(0.6) {
            Some(format!("Review of product {pid} by user {uid}."))
        } else {
            None
        };
        let created_at = now - rng.gen_range(0..(60 * 60 * 24 * 180_i64));
        let res = sqlx::query(
            "INSERT INTO public.reviews(user_id, product_id, rating, title, body, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (user_id, product_id) DO NOTHING",
        )
        .bind(uid)
        .bind(pid)
        .bind(rating)
        .bind(&title)
        .bind(&body)
        .bind(created_at)
        .execute(pool)
        .await?;
        if res.rows_affected() > 0 {
            inserted += 1;
        }
    }
    Ok(())
}

async fn seed_audit_events(
    pool: &PgPool,
    user_ids: &[i64],
    n: u64,
    rng: &mut StdRng,
    now: i64,
) -> sqlx::Result<()> {
    for _ in 0..n {
        let actor = if rng.gen_bool(0.7) {
            let uid = user_ids[rng.gen_range(0..user_ids.len())];
            format!("user:{uid}")
        } else {
            "system".to_string()
        };
        let action = one_of(rng, AUDIT_ACTIONS);
        let entity = one_of(rng, AUDIT_ENTITIES);
        let entity_id: Option<i64> = if rng.gen_bool(0.85) {
            Some(rng.gen_range(1..10_000))
        } else {
            None
        };
        let created_at = now - rng.gen_range(0..(60 * 60 * 24 * 30_i64));
        sqlx::query(
            "INSERT INTO audit.event_log(actor, action, entity, entity_id, created_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&actor)
        .bind(action)
        .bind(entity)
        .bind(entity_id)
        .bind(created_at)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn print_counts(pool: &PgPool) -> sqlx::Result<()> {
    let tables: &[&str] = &[
        "public.users",
        "public.addresses",
        "public.categories",
        "public.products",
        "public.inventory",
        "public.orders",
        "public.order_items",
        "public.payments",
        "public.shipments",
        "public.reviews",
        "audit.event_log",
    ];
    println!("row counts:");
    for t in tables {
        let q = format!("SELECT count(*) AS n FROM {t}");
        let row = sqlx::raw_sql(sqlx::AssertSqlSafe(q.as_str()))
            .fetch_one(pool)
            .await?;
        let n: i64 = row.try_get("n")?;
        println!("  {t:<24} {n}");
    }
    Ok(())
}

// =====================================================================
// main
// =====================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let db_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&db_url)
        .await?;

    let mut rng = StdRng::seed_from_u64(env_u64("SEED_RNG", 42));
    let now = unix_now();

    let n_users = env_u64("SEED_USERS", 120);
    let n_products = env_u64("SEED_PRODUCTS", 240);
    let n_orders = env_u64("SEED_ORDERS", 300);
    let n_reviews = env_u64("SEED_REVIEWS", 250);
    let n_events = env_u64("SEED_EVENTS", 400);

    println!("seeding against {db_url}");
    if !reset_disabled() {
        println!("  - dropping + recreating schemas");
        reset(&pool).await?;
        bootstrap(&pool).await?;
    } else {
        println!("  - SEED_RESET=skip, leaving existing tables in place");
    }

    let user_ids = seed_users(&pool, n_users, &mut rng, now).await?;
    let addresses_by_user = seed_addresses(&pool, &user_ids, &mut rng).await?;
    let category_ids = seed_categories(&pool).await?;
    let product_prices = seed_products(&pool, &category_ids, n_products, &mut rng, now).await?;
    let product_ids: Vec<i64> = product_prices.keys().copied().collect();
    seed_inventory(&pool, &product_ids, &mut rng, now).await?;
    let order_ids = seed_orders(
        &pool,
        &user_ids,
        &addresses_by_user,
        n_orders,
        &mut rng,
        now,
    )
    .await?;
    seed_order_items(&pool, &order_ids, &product_prices, &mut rng).await?;
    backfill_order_totals(&pool).await?;
    seed_payments(&pool, &mut rng, now).await?;
    seed_shipments(&pool, &addresses_by_user, &mut rng).await?;
    seed_reviews(&pool, &user_ids, &product_prices, n_reviews, &mut rng, now).await?;
    seed_audit_events(&pool, &user_ids, n_events, &mut rng, now).await?;

    sqlx::query("DROP VIEW IF EXISTS public.user_order_summary")
        .execute(&pool)
        .await?;
    sqlx::query(USER_ORDER_SUMMARY_VIEW).execute(&pool).await?;

    println!("seed complete");
    print_counts(&pool).await?;
    Ok(())
}
