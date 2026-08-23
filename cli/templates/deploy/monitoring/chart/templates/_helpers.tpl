{{/*
  Environment for the collector container.

  The define name is fixed rather than templated with the project name, unlike
  the application chart's `{{name}}.apiEnv`. Helm's define namespace is per
  render and this chart is its own release, so there is nothing to collide with.

  Only *scalars* belong here. `config_rs::Environment` is built with
  `.separator("__").try_parsing(true)` and no `list_separator`, so a list-valued
  key — `[cors] allowed_origins`, `[metrics] table_counts`, `[jobs.workers.*]
  jobs` — cannot be set from the environment at all. Those live in
  monitoring/config/production.toml, which ships inside the image.
*/}}
{{- define "erno-monitoring.collectorEnv" -}}
- name: DATABASE_URL
  value: {{ .Values.collector.database_url | quote }}
- name: APP__SERVER__PORT
  value: {{ .Values.collector.port | default 3001 | quote }}
- name: APP__API_URL
  value: {{ .Values.collector.api_url | quote }}
- name: APP__AUTH__SECRET
  value: {{ .Values.collector.jwt_secret | quote }}
- name: APP__TRACING__LOG_LEVEL
  value: {{ .Values.collector.log_level | default "info" | quote }}
{{- if .Values.collector.admin_password_hash }}
- name: APP__ADMIN__USERNAME
  value: {{ .Values.collector.admin_username | default "admin" | quote }}
- name: APP__ADMIN__PASSWORD_HASH
  value: {{ .Values.collector.admin_password_hash | quote }}
{{- end }}
{{- if .Values.collector.metrics_auth_token }}
- name: APP__METRICS__AUTH_TOKEN
  value: {{ .Values.collector.metrics_auth_token | quote }}
{{- end }}
- name: APP__COLLECTOR__ENABLED
  value: "true"
{{/* Trusted server-to-server path. Must equal the application chart's
     api.error_reporting.ingest_token, or the API reports to a 401. */}}
- name: APP__COLLECTOR__SERVER_TOKEN
  value: {{ .Values.collector.server_token | quote }}
{{/* PUBLIC — this one ships inside the browser bundle. */}}
- name: APP__COLLECTOR__BROWSER_TOKEN
  value: {{ .Values.collector.browser_token | quote }}
{{- if .Values.collector.alerts_recipient }}
- name: APP__COLLECTOR__ALERTS__ENABLED
  value: "true"
- name: APP__COLLECTOR__ALERTS__RECIPIENT
  value: {{ .Values.collector.alerts_recipient | quote }}
{{- end }}
- name: APP__COLLECTOR__STATUS__ENABLED
  value: {{ .Values.collector.status_enabled | default true | quote }}
- name: APP__COLLECTOR__STATUS__NAME
  value: {{ .Values.collector.status_name | quote }}
- name: APP__COLLECTOR__STATUS__OUTPUT_PATH
  value: "/app/status/status.json"
{{- if .Values.prometheus.enabled }}
{{/* Lets alert rules query PromQL. In-cluster, never through the ingress. */}}
- name: APP__COLLECTOR__PROMETHEUS__URL
  value: "http://{{ .Release.Name }}-prometheus:9090"
{{- end }}
- name: APP__EMAIL__TYPE
  value: "smtp"
- name: APP__EMAIL__HOST
  value: {{ .Values.collector.smtp_host | quote }}
- name: APP__EMAIL__PORT
  value: {{ .Values.collector.smtp_port | default 587 | quote }}
- name: APP__EMAIL__USERNAME
  value: {{ .Values.collector.smtp_username | quote }}
- name: APP__EMAIL__PASSWORD
  value: {{ .Values.collector.smtp_password | quote }}
- name: APP__EMAIL__FROM
  value: {{ .Values.collector.smtp_from | quote }}
{{- end }}
