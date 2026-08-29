# POS System Web 1

This project is a lightweight Python web-based POS system built for Python 3.6 without external dependencies.

## Features

- Server-rendered web UI over `wsgiref`
- JSON-backed products, sales, sale items, and inventory movements
- Seeded users for demo access
- Signed cookie session with cart persistence
- Receipt view and sales history
- Unit tests for login, product creation, and checkout flow

## Run

```bash
python3 run.py
```

The app starts on `http://127.0.0.1:8080`.

## Demo Accounts

- `admin` / `admin123`
- `cashier` / `cashier123`

## Environment Variables

- `POS_DB_PATH` to override the JSON data file location
- `POS_SECRET_KEY` to override the session signing key

## Test

```bash
python3 -m unittest discover -s tests
```
