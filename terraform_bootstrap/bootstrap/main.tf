provider "google" {
  project                         = var.project_id
  region                          = var.region
  add_terraform_attribution_label = false
}

data "google_storage_bucket" "terraform_state" {
  name = var.terraform_state_bucket
}

output "terraform_state_bucket" {
  description = "GCS bucket for Terraform state."
  value       = data.google_storage_bucket.terraform_state.name
}
