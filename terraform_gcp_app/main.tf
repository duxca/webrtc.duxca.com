provider "google" {
  project                         = var.project_id
  region                          = var.region
  add_terraform_attribution_label = false
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
    # No fallback: running as the default compute account would silently grant
    # the app project-wide permissions.
    service_account                  = data.terraform_remote_state.storage.outputs.webrtc_container_service_account_email
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

      dynamic "env" {
        for_each = var.allowed_github_ids == "" ? [] : [var.allowed_github_ids]
        content {
          name  = "ALLOWED_GITHUB_IDS"
          value = env.value
        }
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

# Making the service public and mapping the domain need IAM and domain
# permissions the deployer no longer has; terraform_gcp_edge owns them now.
# These blocks drop them from this state without destroying them.
removed {
  from = google_cloud_run_v2_service_iam_member.public_invoker

  lifecycle {
    destroy = false
  }
}

removed {
  from = google_cloud_run_domain_mapping.webrtc

  lifecycle {
    destroy = false
  }
}
