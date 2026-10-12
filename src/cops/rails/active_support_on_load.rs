use crate::prelude::*;

pub const NAME: &str = "Rails/ActiveSupportOnLoad";
pub static COP: ActiveSupportOnLoad = ActiveSupportOnLoad;
pub struct ActiveSupportOnLoad;

const LOAD_HOOKS: &[(&str, &str)] = &[
    ("ActionCable", "action_cable"),
    ("ActionCable::Channel::Base", "action_cable_channel"),
    ("ActionCable::Connection::Base", "action_cable_connection"),
    ("ActionCable::Connection::TestCase", "action_cable_connection_test_case"),
    ("ActionController::API", "action_controller"),
    ("ActionController::Base", "action_controller"),
    ("ActionController::TestCase", "action_controller_test_case"),
    ("ActionDispatch::IntegrationTest", "action_dispatch_integration_test"),
    ("ActionDispatch::Request", "action_dispatch_request"),
    ("ActionDispatch::Response", "action_dispatch_response"),
    ("ActionDispatch::SystemTestCase", "action_dispatch_system_test_case"),
    ("ActionMailbox::Base", "action_mailbox"),
    ("ActionMailbox::InboundEmail", "action_mailbox_inbound_email"),
    ("ActionMailbox::Record", "action_mailbox_record"),
    ("ActionMailbox::TestCase", "action_mailbox_test_case"),
    ("ActionMailer::Base", "action_mailer"),
    ("ActionMailer::TestCase", "action_mailer_test_case"),
    ("ActionText::Content", "action_text_content"),
    ("ActionText::Record", "action_text_record"),
    ("ActionText::RichText", "action_text_rich_text"),
    ("ActionView::Base", "action_view"),
    ("ActionView::TestCase", "action_view_test_case"),
    ("ActiveJob::Base", "active_job"),
    ("ActiveJob::TestCase", "active_job_test_case"),
    ("ActiveRecord::Base", "active_record"),
    ("ActiveStorage::Attachment", "active_storage_attachment"),
    ("ActiveStorage::Blob", "active_storage_blob"),
    ("ActiveStorage::Record", "active_storage_record"),
    ("ActiveStorage::VariantRecord", "active_storage_variant_record"),
    ("ActiveSupport::TestCase", "active_support_test_case"),
];
const RAILS_5_2: &[(&str, &str)] = &[("ActiveRecord::ConnectionAdapters::SQLite3Adapter", "active_record_sqlite3adapter")];
const RAILS_7_1: &[(&str, &str)] = &[
    ("ActiveRecord::TestFixtures", "active_record_fixtures"),
    ("ActiveModel::Model", "active_model"),
    ("ActionText::EncryptedRichText", "action_text_encrypted_rich_text"),
    ("ActiveRecord::ConnectionAdapters::PostgreSQLAdapter", "active_record_postgresqladapter"),
    ("ActiveRecord::ConnectionAdapters::Mysql2Adapter", "active_record_mysql2adapter"),
    ("ActiveRecord::ConnectionAdapters::TrilogyAdapter", "active_record_trilogyadapter"),
];

impl Cop for ActiveSupportOnLoad {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let method = method_name(&call);
        if !matches!(method, "prepend" | "include" | "extend") {
            return;
        }
        // receiver, method, arguments = *node: `arguments` is the first argument.
        let args = send_args(&call);
        let Some(first) = args.first() else { return };
        let Some(name) = call.receiver().and_then(|r| helpers::const_name::const_name(&r)) else { return };
        let find = |t: &[(&str, &'static str)]| t.iter().find(|(k, _)| *k == name).map(|(_, v)| *v);
        let rails = cx.config.target_rails_version();
        let hook = find(LOAD_HOOKS)
            .or_else(|| if rails >= 5.2 { find(RAILS_5_2) } else { None })
            .or_else(|| if rails >= 7.1 { find(RAILS_7_1) } else { None });
        let Some(hook) = hook else { return };
        let preferred = format!("ActiveSupport.on_load(:{hook}) {{ {method} {} }}", node_text(first));
        let (s, e) = send_range(&call);
        let msg = format!("Use `{preferred}` instead of `{}`.", cx.src.slice(s, e));
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &preferred));
    }
}
