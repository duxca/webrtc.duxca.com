variable "project_id" {
  description = "Google Cloud project ID."
  type        = string
}

variable "region" {
  description = "Google Cloud region."
  type        = string
  default     = "asia-northeast1"
}

variable "terraform_state_bucket" {
  description = "GCS bucket name for Terraform state."
  type        = string
}
