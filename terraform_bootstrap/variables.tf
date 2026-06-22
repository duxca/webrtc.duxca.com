variable "project_id" {
  description = "Google Cloud project ID."
  type        = string
}

variable "project_number" {
  description = "Google Cloud project number."
  type        = string
}

variable "github_repository" {
  description = "GitHub repository allowed to impersonate the deployer service account."
  type        = string
}

variable "workload_identity_pool_id" {
  description = "Workload Identity Pool ID for GitHub Actions."
  type        = string
  default     = "githubaction"
}

variable "workload_identity_provider_id" {
  description = "Workload Identity Provider ID for this repository."
  type        = string
  default     = "github"
}

variable "deployer_service_account_id" {
  description = "GitHub Actions deployer service account ID."
  type        = string
  default     = "github-action-webrtc"
}

variable "plan_service_account_id" {
  description = "GitHub Actions read-only Terraform plan service account ID."
  type        = string
  default     = "github-action-webrtc-plan"
}

variable "terraform_state_bucket" {
  description = "Terraform state bucket name."
  type        = string
  default     = "webrtc-469410-terraform-state"
}

variable "app_service_account_email" {
  description = "Cloud Run runtime service account email."
  type        = string
}
