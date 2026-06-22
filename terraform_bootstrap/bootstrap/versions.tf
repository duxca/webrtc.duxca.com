terraform {
  required_version = ">= 1.5.0"

  backend "gcs" {
    bucket = "webrtc-469410-terraform-state"
    prefix = "webrtc.duxca.com/bootstrap_state"
  }

  required_providers {
    google = {
      source  = "hashicorp/google"
      version = ">= 6.0.0, < 7.0.0"
    }
  }
}
