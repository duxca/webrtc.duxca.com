terraform {
  required_version = ">= 1.7.0" # `removed` blocks

  backend "gcs" {
    bucket = "webrtc-469410-terraform-state"
    prefix = "webrtc.duxca.com/gcp_app"
  }

  required_providers {
    google = {
      source  = "hashicorp/google"
      version = ">= 6.0.0, < 7.0.0"
    }
  }
}
