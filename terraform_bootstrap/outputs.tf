output "deployer_service_account_email" {
  description = "GitHub Actions deployer service account email."
  value       = google_service_account.deployer.email
}

output "plan_service_account_email" {
  description = "GitHub Actions read-only Terraform plan service account email."
  value       = google_service_account.plan.email
}

output "workload_identity_provider" {
  description = "Workload Identity Provider resource name for google-github-actions/auth."
  value       = google_iam_workload_identity_pool_provider.github.name
}
