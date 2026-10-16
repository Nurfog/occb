-- WhatsApp Business API config (singleton)
CREATE TABLE IF NOT EXISTS whatsapp_config (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    phone_number_id VARCHAR(255) NOT NULL,
    business_account_id VARCHAR(255) NOT NULL,
    api_token TEXT NOT NULL,
    webhook_verify_token VARCHAR(255) DEFAULT uuid_generate_v4()::text,
    phone_number VARCHAR(50),
    is_active BOOLEAN DEFAULT true,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Lead assignment strategy config
CREATE TABLE IF NOT EXISTS lead_assignment_config (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    strategy VARCHAR(50) NOT NULL DEFAULT 'round_robin',
    max_active_leads INT DEFAULT 10,
    territory_enabled BOOLEAN DEFAULT false,
    notify_on_assign BOOLEAN DEFAULT true,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Agent lead assignments
CREATE TABLE IF NOT EXISTS agent_lead_assignments (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    agent_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    lead_id UUID NOT NULL,
    lead_type VARCHAR(20) NOT NULL,
    source VARCHAR(50) DEFAULT 'manual',
    assigned_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    status VARCHAR(50) DEFAULT 'active',
    UNIQUE(lead_id, lead_type)
);

-- WhatsApp messages
CREATE TABLE IF NOT EXISTS whatsapp_messages (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    direction VARCHAR(10) NOT NULL,
    from_number VARCHAR(50) NOT NULL,
    to_number VARCHAR(50) NOT NULL,
    content TEXT NOT NULL,
    message_id VARCHAR(255),
    contact_id UUID REFERENCES contacts(id) ON DELETE SET NULL,
    agent_id UUID REFERENCES users(id) ON DELETE SET NULL,
    status VARCHAR(50) DEFAULT 'sent',
    wa_timestamp TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_agent_assignments_agent ON agent_lead_assignments(agent_id);
CREATE INDEX IF NOT EXISTS idx_agent_assignments_lead ON agent_lead_assignments(lead_id, lead_type);
CREATE INDEX IF NOT EXISTS idx_whatsapp_messages_contact ON whatsapp_messages(contact_id);
CREATE INDEX IF NOT EXISTS idx_whatsapp_messages_number ON whatsapp_messages(from_number);
