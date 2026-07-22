# Create DB

```sh
psql -d postgres

CREATE DATABASE "PGAgent" OWNER alexz;

GRANT ALL PRIVILEGES ON DATABASE "PGAgent" TO alexz;
```