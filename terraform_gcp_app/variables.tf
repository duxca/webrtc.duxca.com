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
  description = "Cloud Run service name."
  type        = string
  default     = "webrtc"
}

variable "domain_name" {
  description = "Custom domain mapped to the Cloud Run service."
  type        = string
  default     = "webrtc.duxca.com"
}

variable "container_image" {
  description = "Container image deployed to Cloud Run."
  type        = string
}

variable "github_client_id" {
  description = "GitHub OAuth client ID."
  type        = string
}

variable "redirect_url" {
  description = "GitHub OAuth redirect URL."
  type        = string
  default     = "https://webrtc.duxca.com/oauth/callback"
}

variable "terraform_state_bucket" {
  description = "Terraform remote state bucket."
  type        = string
  default     = "webrtc-469410-terraform-state"
}

variable "storage_state_prefix" {
  description = "Terraform remote state prefix for terraform_gcp_storage."
  type        = string
  default     = "webrtc.duxca.com/gcp_storage"
}
