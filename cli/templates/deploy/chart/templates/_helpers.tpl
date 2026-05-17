{{- define "{{name}}.apiEnv" -}}
- name: DATABASE_URL
  value: {{ .Values.api.database_url | quote }}
- name: APP__SERVER__PORT
  value: {{ .Values.api.port | default 3000 | quote }}
- name: APP__API_URL
  value: {{ .Values.api.api_url | quote }}
- name: APP__AUTH__SECRET
  value: {{ .Values.api.jwt_secret | quote }}
- name: APP__TRACING__LOG_LEVEL
  value: {{ .Values.api.log_level | default "info" | quote }}
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
