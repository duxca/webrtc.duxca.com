variable "project_id" {
  description = "Google Cloud project ID."
  type        = string
}

variable "region" {
  description = "Google Cloud region."
  type        = string
  default     = "asia-northeast1"
}

variable "docker_registry" {
  description = "Docker Artifact Registry repository ID."
  type        = string
  default     = "cloud-run-source-deploy"
}

variable "container_service_account_id" {
  description = "Cloud Run runtime service account ID."
  type        = string
  default     = "webrtc-container"
}

