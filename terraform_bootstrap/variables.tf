variable "project_id" {
  description = "Google Cloud project ID."
  type        = string
}

variable "project_number" {
  description = "Google Cloud project number."
  type        = string
}

variable "region" {
  description = "Region of the Artifact Registry repository."
  type        = string
  default     = "asia-northeast1"
}

variable "github_repository_id" {
  description = "Numeric GitHub repository id allowed to impersonate the deployer (gh api repos/OWNER/REPO --jq .id)."
  type        = string
}

variable "github_repository_owner_id" {
  description = "Numeric GitHub id of the repository owner (gh api repos/OWNER/REPO --jq .owner.id)."
  type        = string
}

variable "github_deploy_environment" {
  description = "GitHub Actions environment of the deploy job."
  type        = string
  default     = "production"
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

variable "terraform_state_bucket" {
  description = "Terraform state bucket name."
  type        = string
  default     = "webrtc-469410-terraform-state"
}

variable "artifact_registry_repository" {
  description = "Artifact Registry repository the deployer pushes images to."
  type        = string
  default     = "cloud-run-source-deploy"
}

variable "github_client_secret_id" {
  description = "Secret Manager secret holding the GitHub OAuth client secret."
  type        = string
  default     = "WEBRTC_GITHUB_CLIENT_SECRET"
}

variable "app_service_account_email" {
  description = "Cloud Run runtime service account email."
  type        = string
}
