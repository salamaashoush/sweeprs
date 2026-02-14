use crate::rules::{CleanupRule, cache_rule};
use crate::scanner::entry::{Category, SafetyLevel};

cache_rule!(
    GcloudCacheRule,
    "gcloud CLI cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".cache/gcloud"
);

cache_rule!(
    AwsCliCacheRule,
    "AWS CLI cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".aws/cli/cache"
);

cache_rule!(
    TerraformPluginCacheRule,
    "Terraform plugin cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".terraform.d/plugin-cache"
);

cache_rule!(
    AzureCliCacheRule,
    "Azure CLI cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".azure/cliextensions",
    ".azure/commands"
);

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(GcloudCacheRule),
        Box::new(AwsCliCacheRule),
        Box::new(TerraformPluginCacheRule),
        Box::new(AzureCliCacheRule),
    ]
}
