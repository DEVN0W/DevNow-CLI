# POS System Web

A lightweight, dependency-free Python POS system built with the standard library.

## Features

- Product catalog with stock levels and price management
- Cart-based checkout flow
- Cash tendering with change calculation
- Sales history with daily summary metrics
- SQLite persistence
- CSRF protection for mutating actions

## Run

```bash
python3 run.py
```

The app starts on `http://127.0.0.1:8000`.

## Test

```bash
python3 -m unittest discover -s tests
```
