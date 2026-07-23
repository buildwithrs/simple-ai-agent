# E-commerce Test Schema

The schema is split across two namespaces — `public` for operational tables
and `audit` for an event log — so every tool in `src/db/` has something to act
on:

- `list_schemas` returns both namespaces.
- `list_tables(schema='public', include_views=true)` returns the tables *and*
  the `user_order_summary` view.
- `desc_table` is exercised against any of the rich tables below.
- `search_schema` has plenty of cross-table overlap (e.g. many tables carry
  `status`, `created_at`, `user_id`) for fuzzy search to surface.
- `execute_query` joins, filters, and aggregates across the schema.
- `execute_dml` hits every DML verb (`INSERT`, `UPDATE`, `DELETE`).
- `execute_ddl` can `ALTER`, `DROP INDEX`, `TRUNCATE`, or `CREATE` against
  the schema without harming the seed flow (the seeder recreates everything
  it owns on every run).

## Conventions

- All surrogate keys are `BIGSERIAL`.
- Money is `BIGINT` cents — no floats anywhere.
- Timestamps are `BIGINT` unix-seconds. Keeps the seeder free of extra
  date-handling dependencies.
- Foreign keys are explicit, with `ON DELETE` actions chosen per relationship.
- Cross-schema data let `list_schemas` and cross-schema `desc_table` be
  exercised end-to-end.

## Tables

### `public.users`

Customers (and a sprinkling of disabled accounts).

| Column     | Type           | Notes                              |
|------------|----------------|------------------------------------|
| id         | BIGSERIAL PK   |                                    |
| email      | TEXT UNIQUE    | Generated unique-per-run.          |
| full_name  | TEXT           | First + last from a small pool.    |
| phone      | TEXT NULL      | ~70% populated.                    |
| status     | TEXT           | `active` (92%) or `disabled`.      |
| created_at | BIGINT         | Up to ~365 days back.              |

Indexes: `idx_users_status`.

### `public.addresses`

One-to-many from `users`. The first address per user is flagged
`is_default=true` to mirror how billing systems break ties.

| Column       | Type          | Notes                                       |
|--------------|---------------|---------------------------------------------|
| id           | BIGSERIAL PK  |                                             |
| user_id      | BIGINT FK     | `users.id` ON DELETE CASCADE.               |
| line1        | TEXT          |                                             |
| line2        | TEXT NULL     | Apt/suite, ~30% populated.                  |
| city         | TEXT          |                                             |
| region       | TEXT          | State/province.                             |
| country      | TEXT          |                                             |
| postal_code  | TEXT          |                                             |
| is_default   | BOOLEAN       | Exactly one true per user (when present).   |

Indexes: `idx_addresses_user`.

### `public.categories`

Tree-structured taxonomy. Top-level nodes (`Books`, `Electronics`,
`Home & Kitchen`, `Apparel`, `Toys & Games`) plus a handful of
sub-categories under each.

| Column      | Type           | Notes                              |
|-------------|----------------|------------------------------------|
| id          | BIGSERIAL PK   |                                    |
| parent_id   | BIGINT NULL FK | `categories.id` ON DELETE SET NULL.|
| name        | TEXT           |                                    |
| slug        | TEXT UNIQUE   | Derived from name.                  |
| description | TEXT NULL      |                                    |

Indexes: `idx_categories_parent`.

### `public.products`

Items sold in the store. Each row belongs to a single category and to a
single inventory record.

| Column      | Type           | Notes                                  |
|-------------|----------------|----------------------------------------|
| id          | BIGSERIAL PK   |                                        |
| category_id | BIGINT FK      | ON DELETE RESTRICT.                    |
| sku         | TEXT UNIQUE   | Zero-padded per batch.                  |
| name        | TEXT          | "Color Size Kind" pattern.              |
| description | TEXT NULL      |                                        |
| price_cents | BIGINT        | CHECK (>= 0).                          |
| currency    | TEXT          | Always `USD` here.                     |
| status      | TEXT          | on_sale / out_of_stock / discontinued. |
| created_at  | BIGINT        |                                        |

Indexes: `idx_products_category`, `idx_products_status`.

### `public.inventory`

One row per product, recording warehouse-level on-hand and reserved
quantities.

| Column     | Type           | Notes                          |
|------------|----------------|--------------------------------|
| product_id | BIGINT PK, FK  | ON DELETE CASCADE.             |
| warehouse  | TEXT          | `main` / `east` / `west`.      |
| on_hand    | BIGINT        | CHECK (>= 0).                  |
| reserved   | BIGINT        | CHECK (>= 0).                  |
| updated_at | BIGINT        |                                |

### `public.orders`

Header of a customer order. `ship_address_id` belongs to the same user as
`user_id` (constraint enforced by the seeder, not by the schema).

| Column          | Type          | Notes                                       |
|-----------------|---------------|---------------------------------------------|
| id              | BIGSERIAL PK  |                                             |
| user_id         | BIGINT FK     | ON DELETE RESTRICT.                         |
| ship_address_id | BIGINT FK     | ON DELETE RESTRICT.                         |
| status          | TEXT          | weighted distribution (see below).          |
| subtotal_cents  | BIGINT        | Backfilled from `order_items`.              |
| shipping_cents  | BIGINT        |                                             |
| total_cents     | BIGINT        | `subtotal_cents + shipping_cents`.          |
| placed_at       | BIGINT        |                                             |

Indexes: `idx_orders_user`, `idx_orders_status`.

### `public.order_items`

Line items. `unit_price_cents` is captured at order time (denormalised
from `products.price_cents`).

| Column            | Type          | Notes                                       |
|-------------------|---------------|---------------------------------------------|
| id                | BIGSERIAL PK  |                                             |
| order_id          | BIGINT FK     | ON DELETE CASCADE.                          |
| product_id        | BIGINT FK     | ON DELETE RESTRICT.                         |
| quantity          | INTEGER       | CHECK (> 0).                                |
| unit_price_cents  | BIGINT        |                                             |
| line_total_cents  | BIGINT        | `quantity * unit_price_cents`.              |

Indexes: `idx_order_items_order`, `idx_order_items_product`.

### `public.payments`

One row per order (`UNIQUE` on `order_id`). Status aligns with the order:
`pending`/`authorized` for pending orders, `captured` for paid/shipped/
delivered, `refunded`/`failed` for cancelled/refunded.

| Column       | Type             | Notes                                       |
|--------------|------------------|---------------------------------------------|
| id           | BIGSERIAL PK     |                                             |
| order_id     | BIGINT UNIQUE FK | ON DELETE CASCADE.                          |
| method       | TEXT             | card / paypal / bank_transfer / cod.        |
| amount_cents | BIGINT           | Mirror of `orders.total_cents`.             |
| status       | TEXT             |                                             |
| provider_ref | TEXT NULL        | Only set when `method` is card or paypal.   |
| captured_at  | BIGINT NULL      |                                             |

### `public.shipments`

One shipment per order that has progressed past `pending`. The
`ship_address_id` belongs to the same user that owns the order.

| Column          | Type          | Notes                                          |
|-----------------|---------------|------------------------------------------------|
| id              | BIGSERIAL PK  |                                                |
| order_id        | BIGINT FK     | ON DELETE CASCADE.                             |
| ship_address_id | BIGINT FK     | ON DELETE RESTRICT.                            |
| carrier         | TEXT          | ups / fedex / dhl / internal.                  |
| tracking_no     | TEXT          |                                                |
| status          | TEXT          | pending / in_transit / delivered / returned.   |
| shipped_at      | BIGINT NULL   |                                                |
| delivered_at    | BIGINT NULL   |                                                |

Indexes: `idx_shipments_order`, `idx_shipments_status`.

### `public.reviews`

Unique-per-user-product review. Ratings skew toward 4–5.

| Column     | Type          | Notes                                  |
|------------|---------------|----------------------------------------|
| id         | BIGSERIAL PK  |                                        |
| user_id    | BIGINT FK     | ON DELETE CASCADE.                     |
| product_id | BIGINT FK     | ON DELETE CASCADE.                     |
| rating     | SMALLINT      | CHECK (BETWEEN 1 AND 5).               |
| title      | TEXT NULL     |                                        |
| body       | TEXT NULL     |                                        |
| created_at | BIGINT        |                                        |

Indexes: `idx_reviews_product`, `UNIQUE (user_id, product_id)`.

### `audit.event_log`

Cross-schema target for `list_schemas` and `desc_table("audit.event_log")`.

| Column    | Type           | Notes                          |
|-----------|----------------|--------------------------------|
| id        | BIGSERIAL PK   |                                |
| actor     | TEXT           | `user:<id>` or `system`.       |
| action    | TEXT           | login / create / update / ...  |
| entity    | TEXT           | user / order / product / ...   |
| entity_id | BIGINT NULL    |                                |
| created_at| BIGINT         |                                |

Indexes: `idx_event_log_actor`.

## View

`public.user_order_summary` (regular, non-materialised) aggregates each
user's lifetime spend; useful for `execute_query`.

```sql
CREATE VIEW public.user_order_summary AS
SELECT u.id AS user_id,
       u.email,
       u.full_name,
       COUNT(o.id)        AS order_count,
       COALESCE(SUM(o.total_cents), 0) AS lifetime_cents
FROM public.users u
LEFT JOIN public.orders o ON o.user_id = u.id
GROUP BY u.id, u.email, u.full_name;
```

## Relationships

```
                  ┌────────────┐
                  │ categories │──self-ref (parent_id)
                  └─────┬──────┘
                        │ 1..N
                        ▼
                  ┌────────────┐
                  │  products  │
                  └─────┬──────┘
                        │ 1..1
                        ▼
                  ┌────────────┐
                  │ inventory  │
                  └────────────┘

   ┌────────┐ 1     N ┌────────┐ N     1 ┌───────────┐
   │ users  │────────▶│ orders │────────▶│ addresses │
   └──┬─────┘        └────┬───┘         └───────────┘
      │ 1                 │ 1
      │ N                 │ N
      ▼                   ▼
   ┌─────────┐      ┌──────────┐
   │addresses│      │ payments │ (UNIQUE order_id)
   └─────────┘      └──────────┘

   orders ──1..N──▶ order_items ──▶ products
   users  ──N─────▶ reviews   ──N────▶ products
   orders ──N─────▶ shipments ────────▶ addresses

   audit.event_log  (actor references users by id, no FK)
```

## How to seed

The seeder is `src/bin/seed.rs`. It is **idempotent** and **deterministic**
given the seed.

```sh
cargo run --bin seed                       # default scale
SEED_USERS=200 SEED_PRODUCTS=400 SEED_RNG=7 cargo run --bin seed
```

Environment knobs (all optional, all have sensible defaults):

| Var             | Default | Purpose                                  |
|-----------------|---------|------------------------------------------|
| `DATABASE_URL`  | —       | Required. Postgres connection string.    |
| `SEED_RESET`    | `true`  | Set to `skip` to leave existing tables.  |
| `SEED_RNG`      | `42`    | RNG seed (reproducible golden tests).     |
| `SEED_USERS`    | `120`   | Number of customers.                     |
| `SEED_PRODUCTS` | `240`   | Number of products.                      |
| `SEED_ORDERS`   | `300`   | Number of orders.                        |
| `SEED_REVIEWS`  | `250`   | Target review count (deduped).           |
| `SEED_EVENTS`   | `400`   | Audit event count.                       |

After seeding, the binary prints row counts for every table so you can
sanity-check the run.

## Tool -> fixture coverage

| Tool            | Where to look                                                                                                          |
|-----------------|------------------------------------------------------------------------------------------------------------------------|
| `list_schemas`  | `public`, `audit`.                                                                                                     |
| `list_tables`   | 10 tables in `public`; with `include_views=true` also `user_order_summary`.                                            |
| `desc_table`    | `public.orders` (richest: 5 FK columns, multiple indexes), `public.products`, `audit.event_log`.                        |
| `search_schema` | Many columns named `status`, `created_at`, `user_id` spread across tables.                                             |
| `execute_query` | `SELECT * FROM public.user_order_summary ORDER BY lifetime_cents DESC LIMIT 10;` and friends.                          |
| `execute_dml`   | `UPDATE public.products SET status='discontinued' WHERE id=$1;`, `INSERT INTO ...`, `DELETE FROM public.reviews WHERE rating = 1;`. |
| `execute_ddl`   | `ALTER TABLE public.products ADD COLUMN tags TEXT;`, `DROP INDEX idx_orders_status;`, `TRUNCATE TABLE public.payments;`. |
