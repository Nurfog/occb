-- Revert deals.value from DOUBLE PRECISION to NUMERIC for monetary precision
ALTER TABLE deals ALTER COLUMN value TYPE NUMERIC(15,2) USING value::NUMERIC(15,2);
