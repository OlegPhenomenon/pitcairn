-- 0301_slice_c_indexes.sql — slice C (deliverables/results/catalog).
-- The base schema (0001_init.sql) is final; this only adds secondary indexes
-- on tables it already created, for the query patterns introduced by
-- deliverable submission/publication, link checking and the public catalog.

CREATE INDEX idx_external_links_submission ON external_links(submission_id);
CREATE INDEX idx_external_links_available ON external_links(available);
CREATE INDEX idx_submission_files_version ON submission_files(document_version_id);
CREATE INDEX idx_publication_files_version ON publication_files(document_version_id);
CREATE INDEX idx_due_changes_deliverable ON deliverable_due_changes(deliverable_id);
CREATE INDEX idx_measurements_deliverable ON measurements(deliverable_id);
CREATE INDEX idx_deliverables_public ON deliverables(project_id, publish_level);
