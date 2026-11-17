\set ON_ERROR_STOP on
\if :{?apply}
\else
\set apply false
\endif

-- 用 CPR 3.44.0 的 PostgreSQL；先在 CPR UI 停用两个实例，再停止 CPR 服务。
-- psql -X -v source=<旧实例 UUID> -v target=<面板实例 UUID> -f scripts/migrate-turnstate.sql
-- 加 -v apply=true 提交；默认执行同样的检查和修改后回滚。
BEGIN;
SET LOCAL lock_timeout = '5s';

CREATE TEMP TABLE turnstate_cutover (source_id uuid, target_id uuid) ON COMMIT DROP;
INSERT INTO turnstate_cutover VALUES (:'source'::uuid, :'target'::uuid);

DO $$
DECLARE
    src uuid;
    dst uuid;
    probe_namespaces constant text[] := ARRAY['settings', 'history', 'runs'];
    probe_fields constant text[] := ARRAY[
        'probeEnabled', 'minIntervalMinutes', 'fingerprintSyncHours', 'telegramEnabled',
        'telegramChatId', 'telegramThreadId', 'modeltraceClientKeyId'
    ];
    pair record;
    copied bigint;
BEGIN
    SELECT source_id, target_id INTO STRICT src, dst FROM turnstate_cutover;
    PERFORM id FROM plugin_instances WHERE id IN (src, dst) ORDER BY id FOR UPDATE;

    IF NOT EXISTS (
        SELECT 1 FROM plugin_instances i JOIN plugin_artifacts a ON a.sha256 = i.artifact_sha256
        WHERE i.id = src AND NOT i.enabled AND a.plugin_id = 'milesians.cpr-turnstate'
    ) OR NOT EXISTS (
        SELECT 1 FROM plugin_instances i JOIN plugin_artifacts a ON a.sha256 = i.artifact_sha256
        WHERE i.id = dst AND NOT i.enabled AND a.plugin_id = 'milesians.cpr-chatgpt-account-panel'
    ) THEN
        RAISE EXCEPTION '请指定正确的实例 UUID，并先停用旧探测和账号面板实例';
    END IF;

    IF EXISTS (
        SELECT 1 FROM plugin_state_generations
        WHERE instance_id IN (src, dst) AND status = 'staging'
    ) THEN
        RAISE EXCEPTION '插件有进行中的状态迁移，请先完成升级/回滚';
    END IF;
    IF EXISTS (SELECT 1 FROM plugin_account_group_resources WHERE instance_id = dst) THEN
        RAISE EXCEPTION '目标面板已有自建分组，拒绝覆盖';
    END IF;

    FOR pair IN
        SELECT n.namespace, s.id AS source_generation, t.id AS target_generation,
               s.record_count, s.total_bytes, s.next_record_version,
               t.maximum_records, t.maximum_bytes, t.maximum_value_bytes,
               t.record_count AS target_count,
               (s.schema_version = t.schema_version AND s.schema_sha256 = t.schema_sha256) AS same_schema
        FROM unnest(probe_namespaces) AS n(namespace)
        LEFT JOIN plugin_state_generations s
            ON s.instance_id = src AND s.namespace = n.namespace AND s.status = 'active'
        LEFT JOIN plugin_state_generations t
            ON t.instance_id = dst AND t.namespace = n.namespace AND t.status = 'active'
    LOOP
        IF pair.source_generation IS NULL OR pair.target_generation IS NULL OR pair.same_schema IS NOT TRUE THEN
            RAISE EXCEPTION '命名空间 % 缺失或结构不同，请确认旧版和升级后的面板已成功启动过', pair.namespace;
        END IF;
        IF pair.target_count <> 0 OR EXISTS (
            SELECT 1 FROM plugin_state_records WHERE generation_id = pair.target_generation
        ) THEN
            RAISE EXCEPTION '目标命名空间 % 已有数据，拒绝覆盖', pair.namespace;
        END IF;
        IF pair.record_count > pair.maximum_records OR pair.total_bytes > pair.maximum_bytes OR EXISTS (
            SELECT 1 FROM plugin_state_records
            WHERE generation_id = pair.source_generation AND value_bytes > pair.maximum_value_bytes
        ) THEN
            RAISE EXCEPTION '源命名空间 % 超过目标配额', pair.namespace;
        END IF;

        INSERT INTO plugin_state_records (generation_id, state_key, value_json, value_bytes, record_version)
        SELECT pair.target_generation, state_key, value_json, value_bytes, record_version
        FROM plugin_state_records WHERE generation_id = pair.source_generation;
        GET DIAGNOSTICS copied = ROW_COUNT;
        IF copied <> pair.record_count THEN
            RAISE EXCEPTION '源命名空间 % 的记录数与元数据不一致', pair.namespace;
        END IF;
        UPDATE plugin_state_generations
        SET record_count = copied, total_bytes = pair.total_bytes,
            next_record_version = greatest(next_record_version, pair.next_record_version)
        WHERE id = pair.target_generation;
        RAISE NOTICE '%: % 条记录', pair.namespace, copied;
    END LOOP;

    -- 只转移归属，组 ID、成员和已绑定的 Client Key 保持原样。
    UPDATE plugin_account_group_resources SET instance_id = dst WHERE instance_id = src;
    GET DIAGNOSTICS copied = ROW_COUNT;
    RAISE NOTICE '接管 % 个探测插件自建分组', copied;

    UPDATE plugin_instances t
    SET configuration_json = t.configuration_json || coalesce((
        SELECT jsonb_object_agg(k, v)
        FROM jsonb_each(s.configuration_json) AS fields(k, v) WHERE k = ANY(probe_fields)
    ), '{}'::jsonb) || '{"probeMaintenanceEnabled":false}'::jsonb
    FROM plugin_instances s WHERE s.id = src AND t.id = dst;

    -- 只复制 TG Token，继续放在独立敏感配置；输出里不显示配置值。
    INSERT INTO plugin_instance_secrets (instance_id, secrets_json)
    SELECT dst, jsonb_build_object('telegramBotToken', secrets_json->'telegramBotToken')
    FROM plugin_instance_secrets WHERE instance_id = src AND secrets_json ? 'telegramBotToken'
    ON CONFLICT (instance_id) DO UPDATE
    SET secrets_json = plugin_instance_secrets.secrets_json || excluded.secrets_json;
END $$;

\if :apply
COMMIT;
\else
ROLLBACK;
\echo 预演通过，数据库未改动。核对后加 -v apply=true 再执行。
\endif
