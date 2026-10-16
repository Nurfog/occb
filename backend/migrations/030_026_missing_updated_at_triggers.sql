DO $$ DECLARE t text; triggers text[] := ARRAY[
    'contacts:trg_contacts_updated_at',
    'deals:trg_deals_updated_at',
    'activities:trg_activities_updated_at',
    'branding:trg_branding_updated_at',
    'calendar_events:trg_calendar_events_updated_at'
]; parts text[]; tbl text; trg text;
BEGIN
    FOREACH t IN ARRAY triggers LOOP
        parts := string_to_array(t, ':'); tbl := parts[1]; trg := parts[2];
        IF to_regclass(tbl) IS NOT NULL AND NOT EXISTS (
            SELECT 1 FROM pg_trigger WHERE tgname = trg AND tgrelid = tbl::regclass
        ) THEN
            EXECUTE format('CREATE TRIGGER %I BEFORE UPDATE ON %I FOR EACH ROW EXECUTE FUNCTION set_updated_at()', trg, tbl);
        END IF;
    END LOOP;
END $$;
