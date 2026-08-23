{{- define "{{name}}.apiEnv" -}}
- name: DATABASE_URL
  value: {{ .Values.api.database_url | quote }}
- name: APP__SERVER__PORT
  value: {{ .Values.api.port | default 3000 | quote }}
- name: APP__API_URL
  value: {{ .Values.api.api_url | quote }}
- name: APP__AUTH__SECRET
  value: {{ .Values.api.jwt_secret | quote }}
{{- if .Values.api.admin_password_hash }}
- name: APP__ADMIN__PASSWORD_HASH
  value: {{ .Values.api.admin_password_hash | quote }}
- name: APP__ADMIN__USERNAME
  value: "admin"
{{- end }}
{{- if .Values.api.metrics_auth_token }}
- name: APP__METRICS__AUTH_TOKEN
  value: {{ .Values.api.metrics_auth_token | quote }}
{{- end }}
- name: APP__TRACING__LOG_LEVEL
  value: {{ .Values.api.log_level | default "info" | quote }}
{{/*
  Error and subsystem-health reporting to the monitoring deployment.

  Both are driven by these two keys — `spawn_health_reporter` derives its
  endpoint and token from `error_reporting`, so there is no third key to set.
  Without them the whole monitoring platform is dark in production: the API
  reports nowhere and nothing errors to say so.

  `ingest_token` MUST equal the monitoring chart's collector.server_token.
*/}}
{{- if .Values.api.error_reporting.collector_url }}
- name: APP__ERROR_REPORTING__COLLECTOR_URL
  value: {{ .Values.api.error_reporting.collector_url | quote }}
- name: APP__ERROR_REPORTING__INGEST_TOKEN
  value: {{ .Values.api.error_reporting.ingest_token | quote }}
{{- end }}
- name: APP__EMAIL__TYPE
  value: "smtp"
- name: APP__EMAIL__HOST
  value: {{ .Values.api.smtp_host | quote }}
- name: APP__EMAIL__PORT
  value: {{ .Values.api.smtp_port | default 587 | quote }}
- name: APP__EMAIL__USERNAME
  value: {{ .Values.api.smtp_username | quote }}
- name: APP__EMAIL__PASSWORD
  value: {{ .Values.api.smtp_password | quote }}
- name: APP__EMAIL__FROM
  value: {{ .Values.api.smtp_from | quote }}
{{- end }}
