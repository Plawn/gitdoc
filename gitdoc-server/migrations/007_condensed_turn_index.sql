-- Track which turns have been folded into condensed_context.
-- All turns with turn_index <= condensed_up_to are already summarized.
-- Default -1 means no turns have been condensed (all are "recent").

ALTER TABLE conversations ADD COLUMN condensed_up_to INT NOT NULL DEFAULT -1;
