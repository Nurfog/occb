-- Change deals.value from DECIMAL(15,2) to DOUBLE PRECISION to match Rust f64 type
ALTER TABLE deals ALTER COLUMN value TYPE DOUBLE PRECISION;