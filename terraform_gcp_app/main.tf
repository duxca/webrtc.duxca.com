provider "google" {
  project                         = var.project_id
  region                          = var.region
  add_terraform_attribution_label = false
}

locals {
  runtime_service_account_email = try(
    data.terraform_remote_state.storage.outputs.webrtc_container_service_account_email,
    "${data.google_project.current.number}-compute@developer.gserviceaccount.com",
  )
}

resource "google_cloud_run_v2_service" "webrtc" {
  project  = var.project_id
  location = var.region
  name     = var.service_name
  ingress  = "INGRESS_TRAFFIC_ALL"

  scaling {
    manual_instance_count = 0
    min_instance_count    = 0
  }

  template {
    service_account                  = local.runtime_service_account_email
    timeout                          = "3s"
    max_instance_request_concurrency = 128
    execution_environment            = "EXECUTION_ENVIRONMENT_GEN1"

    scaling {
      min_instance_count = 0
      max_instance_count = 1
    }

    containers {
      image = var.container_image

      ports {
        name           = "http1"
        container_port = 8080
      }

      env {
        name  = "HOST_ADDR"
        value = "0.0.0.0:8080"
      }

      env {
        name  = "GITHUB_CLIENT_ID"
        value = var.github_client_id
      }

      env {
        name = "GITHUB_CLIENT_SECRET"
        value_source {
          secret_key_ref {
            secret  = "WEBRTC_GITHUB_CLIENT_SECRET"
            version = "latest"
          }
        }
      }

      env {
        name  = "REDIRECT_URL"
        value = var.redirect_url
      }

      env {
        name  = "LOCAL_CLIENT_ID"
        value = "local"
      }

      env {
        name  = "LOCAL_CLIENT_SECRET"
        value = "local"
      }

      env {
        name  = "LOCAL_REDIRECT_URL"
        value = "http://127.0.0.1:8080/oauth/callback"
      }

      resources {
        limits = {
          cpu    = "1"
          memory = "256Mi"
        }

        cpu_idle          = true
        startup_cpu_boost = false
      }

      startup_probe {
        failure_threshold = 1
        period_seconds    = 240
        timeout_seconds   = 240

        tcp_socket {
          port = 8080
        }
      }
    }
  }

  traffic {
    type    = "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST"
    percent = 100
  }

  lifecycle {
    ignore_changes = [
      client,
      client_version,
    ]
  }
}

resource "google_cloud_run_v2_service_iam_member" "public_invoker" {
  project  = var.project_id
  location = google_cloud_run_v2_service.webrtc.location
  name     = google_cloud_run_v2_service.webrtc.name
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
    route_name       = google_cloud_run_v2_service.webrtc.name
    certificate_mode = "AUTOMATIC"
  }
}
