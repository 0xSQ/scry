// deploy.rhai
let base_domain = "example.com";
let env = "prod"; // "dev", "staging", or "prod"

#{
    target: `app.${base_domain}`,
    environment: env,
    retries: if env == "prod" { 5 } else { 3 },
    timeout_secs: if env == "prod" { 120 } else { 30 },
    notify: #{
        slack_url: "https://hooks.slack.com/myhook",
        on_failure_only: env == "prod",
    }
}
