CREATE TABLE IF NOT EXISTS freight_price (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    volume REAL NOT NULL,
    size REAL NOT NULL,
    type_transport TEXT NOT NULL CHECK (type_transport IN ('boat', 'truck', 'rail')),
    base_price REAL NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO freight_price (volume, size, type_transport, base_price) VALUES (1.0, 1.0, 'truck', 100.0);
