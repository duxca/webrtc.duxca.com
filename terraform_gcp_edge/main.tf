# Public exposure of the Cloud Run service. Applied by an administrator: the
# GitHub Actions deployer has no permission to change IAM policies or domain
# mappings.

provider "google" {
  project                         = var.project_id
  region                          = var.region
  add_terraform_attribution_label = false
}

data "google_project" "current" {
  project_id = var.project_id
}

resource "google_cloud_run_v2_service_iam_member" "public_invoker" {
  project  = var.project_id
  location = var.region
  name     = var.service_name
  role     = "roles/run.invoker"
  member   = "allUsers"
}

resource "google_cloud_run_domain_mapping" "webrtc" {
  project  = data.google_project.current.number
  location = var.region
  name     = var.domain_name

  metadata {
    namespace = data.google_project.current.number
  }

  spec {
    route_name       = var.service_name
    certificate_mode = "AUTOMATIC"
  }
}
