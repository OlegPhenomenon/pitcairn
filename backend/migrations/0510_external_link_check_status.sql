-- External link checks keep a real outcome instead of a boolean (§5 item 11):
-- available | missing | unreachable | login_required | unchecked, plus the
-- HTTP status code (when a response arrived) and a short reason.
ALTER TABLE external_links ADD COLUMN check_status TEXT NOT NULL DEFAULT 'unchecked'
    CHECK (check_status IN ('unchecked', 'available', 'missing', 'unreachable', 'login_required'));
ALTER TABLE external_links ADD COLUMN check_http_status INTEGER;
ALTER TABLE external_links ADD COLUMN check_reason TEXT NOT NULL DEFAULT '';

-- Existing rows: the old check only knew "reachable or not".
UPDATE external_links
SET check_status = CASE available
        WHEN 1 THEN 'available'
        WHEN 0 THEN 'unreachable'
        ELSE 'unchecked'
    END,
    check_reason = CASE available
        WHEN 1 THEN 'Reachable (earlier check)'
        WHEN 0 THEN 'Unavailable (earlier check)'
        ELSE ''
    END;

DROP INDEX idx_external_links_available;
ALTER TABLE external_links DROP COLUMN available;
ALTER TABLE external_links DROP COLUMN last_status;
CREATE INDEX idx_external_links_check_status ON external_links(check_status);
