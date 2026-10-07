-- 0500_independent_permits.sql — a project may hold several independent
-- permits (spec §5 item 5, §6 "Проверка изменений"). Each permit starts its
-- own chain; amendments, extensions and revocations supersede one decision
-- of ONE chain and never touch the other chains.
--
-- title    — human name of the permit ("Reef transect sampling"); amendments
--            inherit it when left empty.
-- chain_id — id of the permit that started the chain; NULL for drafts and
--            refusals. Set when the decision is issued.

ALTER TABLE decisions ADD COLUMN title TEXT NOT NULL DEFAULT '';
ALTER TABLE decisions ADD COLUMN chain_id TEXT REFERENCES decisions(id);

WITH RECURSIVE chain(id, root) AS (
    SELECT id, id FROM decisions
     WHERE status = 'issued' AND supersedes_id IS NULL
       AND kind IN ('permit', 'amendment', 'extension', 'revocation')
    UNION ALL
    SELECT d.id, chain.root FROM decisions d JOIN chain ON d.supersedes_id = chain.id
     WHERE d.status = 'issued'
)
UPDATE decisions SET chain_id = (SELECT root FROM chain WHERE chain.id = decisions.id)
 WHERE id IN (SELECT id FROM chain);

UPDATE decisions SET title = 'Research permit' WHERE chain_id IS NOT NULL AND title = '';
CREATE INDEX idx_decisions_chain ON decisions(project_id, chain_id);
