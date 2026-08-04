INSERT INTO acp_session_usage (
  project_id, chat_id, session_id, turn_count, item_count, item_bytes,
  fact_count, fact_bytes, payload_bytes, revision
)
SELECT s.project_id, s.chat_id, s.id,
       (SELECT COUNT(*) FROM turns t WHERE t.session_id=s.id),
       (SELECT COUNT(*) FROM items i WHERE i.session_id=s.id),
       COALESCE((SELECT SUM(payload_bytes) FROM items i WHERE i.session_id=s.id),0),
       (SELECT COUNT(*) FROM session_facts f WHERE f.session_id=s.id),
       COALESCE((SELECT SUM(payload_bytes) FROM session_facts f WHERE f.session_id=s.id),0),
       COALESCE((SELECT SUM(payload_bytes) FROM items i WHERE i.session_id=s.id),0)
         + COALESCE((SELECT SUM(payload_bytes) FROM session_facts f WHERE f.session_id=s.id),0),
       0
FROM agent_sessions s;

INSERT INTO acp_project_usage (
  project_id, chat_count, session_count, turn_count, item_count,
  fact_count, payload_bytes, revision
)
SELECT p.id,
       (SELECT COUNT(*) FROM chats c WHERE c.project_id=p.id),
       (SELECT COUNT(*) FROM agent_sessions s WHERE s.project_id=p.id),
       (SELECT COUNT(*) FROM turns t WHERE t.project_id=p.id),
       (SELECT COUNT(*) FROM items i WHERE i.project_id=p.id),
       (SELECT COUNT(*) FROM session_facts f WHERE f.project_id=p.id),
       COALESCE((SELECT SUM(payload_bytes) FROM acp_session_usage u WHERE u.project_id=p.id),0),
       0
FROM projects p;

INSERT INTO acp_store_usage (
  singleton, chat_count, session_count, turn_count, item_count,
  item_bytes, fact_count, fact_bytes, shared_payload_bytes, revision
)
SELECT 1,
       (SELECT COUNT(*) FROM chats),
       (SELECT COUNT(*) FROM agent_sessions),
       (SELECT COUNT(*) FROM turns),
       (SELECT COUNT(*) FROM items),
       COALESCE((SELECT SUM(payload_bytes) FROM items),0),
       (SELECT COUNT(*) FROM session_facts),
       COALESCE((SELECT SUM(payload_bytes) FROM session_facts),0),
       COALESCE((SELECT SUM(payload_bytes) FROM items),0)
         + COALESCE((SELECT SUM(payload_bytes) FROM session_facts),0),
       0;
