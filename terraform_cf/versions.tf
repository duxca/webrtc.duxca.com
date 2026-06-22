terraform {
  required_version = ">= 1.5.0"

  backend "gcs" {
    bucket = "webrtc-469410-terraform-state"
    prefix = "webrtc.duxca.com/cf"
  }

  required_providers {
    cloudflare = {
      source  = "cloudflare/cloudflare"
      version = "~> 5.19.1"
    }
  }
}
