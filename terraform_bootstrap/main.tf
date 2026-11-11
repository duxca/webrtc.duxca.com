provider "google" {
  project                         = var.project_id
  add_terraform_attribution_label = false
}

locals {
  deployer_service_account_email = "${var.deployer_service_account_id}@${var.project_id}.iam.gserviceaccount.com"
  deployer_member                = "serviceAccount:${local.deployer_service_account_email}"
  # Numeric ids cannot be re-registered by someone else after a rename or deletion.
  github_repository_principal = "principalSet://iam.googleapis.com/projects/${var.project_number}/locations/global/workloadIdentityPools/${var.workload_identity_pool_id}/attribute.repository_id/${var.github_repository_id}"
}

resource "google_service_account" "deployer" {
  account_id   = var.deployer_service_account_id
  display_name = "GitHub Actions WebRTC deployer"
}

resource "google_iam_workload_identity_pool" "github_actions" {
  workload_identity_pool_id = var.workload_identity_pool_id
  display_name              = "GitHub Actions"
}

resource "google_iam_workload_identity_pool_provider" "github" {
  workload_identity_pool_id          = google_iam_workload_identity_pool.github_actions.workload_identity_pool_id
  workload_identity_pool_provider_id = var.workload_identity_provider_id
  display_name                       = "GitHub"

  attribute_mapping = {
    "google.subject"                = "assertion.sub"
    "attribute.repository"          = "assertion.repository"
    "attribute.repository_id"       = "assertion.repository_id"
    "attribute.repository_owner_id" = "assertion.repository_owner_id"
    "attribute.actor"               = "assertion.actor"
    "attribute.event_name"          = "assertion.event_name"
    "attribute.ref"                 = "assertion.ref"
    "attribute.workflow"            = "assertion.workflow"
  }

  attribute_condition = <<-EOT
    attribute.repository_id == '${var.github_repository_id}' &&
    attribute.repository_owner_id == '${var.github_repository_owner_id}' &&
    attribute.event_name == 'push' &&
    attribute.ref == 'refs/heads/main' &&
    attribute.workflow == 'Deploy to Main' &&
    assertion.environment == '${var.github_deploy_environment}'
  EOT

  oidc {
    issuer_uri = "https://token.actions.githubusercontent.com"
  }
}

resource "google_service_account_iam_member" "github_actions_impersonation" {
  service_account_id = google_service_account.deployer.name
  role               = "roles/iam.workloadIdentityUser"
  member             = local.github_repository_principal
}

# The deployer only rolls out new revisions of the existing Cloud Run service.
# APIs, the runtime service account, Artifact Registry, the secret, the public
# invoker binding and the domain mapping are applied by an administrator
# (terraform_gcp_storage, terraform_gcp_edge).

resource "google_project_iam_member" "deployer_run_developer" {
  project = var.project_id
  role    = "roles/run.developer"
  member  = local.deployer_member
}

resource "google_service_account_iam_member" "app_service_account_user" {
  service_account_id = "projects/${var.project_id}/serviceAccounts/${var.app_service_account_email}"
  role               = "roles/iam.serviceAccountUser"
  member             = local.deployer_member

  depends_on = [google_service_account.deployer]
}

resource "google_artifact_registry_repository_iam_member" "deployer_image_push" {
  project    = var.project_id
  location   = var.region
  repository = var.artifact_registry_repository
  role       = "roles/artifactregistry.writer"
  member     = local.deployer_member

  depends_on = [google_service_account.deployer]
}

# Lets the workflow check that a secret version exists without reading it.
resource "google_secret_manager_secret_iam_member" "deployer_secret_metadata" {
  project   = var.project_id
  secret_id = var.github_client_secret_id
  role      = "roles/secretmanager.viewer"
  member    = local.deployer_member

  depends_on = [google_service_account.deployer]
}

resource "google_storage_bucket_iam_member" "terraform_state_object_admin" {
  bucket = var.terraform_state_bucket
  role   = "roles/storage.objectAdmin"
  member = local.deployer_member

  depends_on = [google_service_account.deployer]
}
