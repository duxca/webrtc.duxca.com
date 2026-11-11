variable "project_id" {
  description = "Google Cloud project ID."
  type        = string
}

variable "region" {
  description = "Google Cloud region."
  type        = string
  default     = "asia-northeast1"
}

variable "service_name" {
  description = "Cloud Run service deployed by terraform_gcp_app."
  type        = string
  default     = "webrtc"
}

variable "domain_name" {
  description = "Custom domain mapped to the Cloud Run service."
  type        = string
  default     = "webrtc.duxca.com"
}
