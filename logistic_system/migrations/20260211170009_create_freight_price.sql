CREATE TABLE IF NOT EXISTS freight_price (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    volume REAL NOT NULL,
    size REAL NOT NULL,
    type_transport TEXT NOT NULL CHECK (type_transport IN ('boat', 'truck', 'rail')),
    base_price REAL NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
