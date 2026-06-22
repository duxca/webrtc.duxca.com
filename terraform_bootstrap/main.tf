provider "google" {
  project                         = var.project_id
  add_terraform_attribution_label = false
}

locals {
  deployer_service_account_email = "${var.deployer_service_account_id}@${var.project_id}.iam.gserviceaccount.com"
  deployer_member                = "serviceAccount:${local.deployer_service_account_email}"
  plan_service_account_email     = "${var.plan_service_account_id}@${var.project_id}.iam.gserviceaccount.com"
  plan_member                    = "serviceAccount:${local.plan_service_account_email}"
  github_repository_principal    = "principalSet://iam.googleapis.com/projects/${var.project_number}/locations/global/workloadIdentityPools/${var.workload_identity_pool_id}/attribute.repository/${var.github_repository}"

  project_roles = toset([
    "roles/artifactregistry.admin",
    "roles/artifactregistry.writer",
    "roles/iam.serviceAccountAdmin",
    "roles/iam.serviceAccountUser",
    "roles/run.admin",
    "roles/secretmanager.admin",
    "roles/serviceusage.serviceUsageAdmin",
    "roles/storage.admin",
  ])
}

resource "google_service_account" "deployer" {
  account_id   = var.deployer_service_account_id
  display_name = "GitHub Actions WebRTC deployer"
}

resource "google_service_account" "plan" {
  account_id   = var.plan_service_account_id
  display_name = "GitHub Actions WebRTC Terraform plan"
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
    "google.subject"       = "assertion.sub"
    "attribute.repository" = "assertion.repository"
    "attribute.actor"      = "assertion.actor"
    "attribute.event_name" = "assertion.event_name"
    "attribute.ref"        = "assertion.ref"
    "attribute.workflow"   = "assertion.workflow"
  }

  attribute_condition = <<-EOT
    attribute.repository == '${var.github_repository}' &&
    attribute.event_name == 'push' &&
    attribute.ref == 'refs/heads/main' &&
    attribute.workflow == 'Deploy to Main'
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

resource "google_service_account_iam_member" "github_actions_plan_impersonation" {
  service_account_id = google_service_account.plan.name
  role               = "roles/iam.workloadIdentityUser"
  member             = local.github_repository_principal
}

resource "google_project_iam_member" "deployer_project_roles" {
  for_each = local.project_roles

  project = var.project_id
  role    = each.value
  member  = local.deployer_member
}

resource "google_storage_bucket_iam_member" "terraform_state_object_admin" {
  bucket = var.terraform_state_bucket
  role   = "roles/storage.objectAdmin"
  member = local.deployer_member

  depends_on = [google_service_account.deployer]
}

resource "google_project_iam_member" "plan_project_viewer" {
  project = var.project_id
  role    = "roles/viewer"
  member  = local.plan_member
}

resource "google_storage_bucket_iam_member" "plan_terraform_state_object_viewer" {
  bucket = var.terraform_state_bucket
  role   = "roles/storage.objectViewer"
  member = local.plan_member

  depends_on = [google_service_account.plan]
}

resource "google_service_account_iam_member" "app_service_account_user" {
  service_account_id = "projects/${var.project_id}/serviceAccounts/${var.app_service_account_email}"
  role               = "roles/iam.serviceAccountUser"
  member             = local.deployer_member

  depends_on = [google_service_account.deployer]
}
