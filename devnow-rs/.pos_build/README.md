# POS System Web

A dependency-free Python POS web app for a small retail store.

## Features

- Dashboard with product count, recent sales, revenue, and low-stock view
- Product catalog management
- Inventory adjustments with movement history
- Checkout with tax, payment method, tendered amount, and change due
- Receipt view for completed sales
- Basic reports for revenue and top products
- JSON file persistence with demo seed data

## Run

```bash
python3 run.py
```

Then open `http://127.0.0.1:8000`.

## Notes

- Data is stored in `data/pos_data.json`.
- The first run also creates `data/admin_pin.txt` with owner-only permissions. Use that staff passcode on the login page.
- Session cookies are signed with `data/session_secret.txt`.
- The app uses only the Python standard library, so it can run on older Python environments.
