# RuboCop 1.91.0's RemoteConfig (lib/rubocop/remote_config.rb) and the cache root it is given
# (ConfigLoader.cache_root, CacheConfig.root_dir), run by src/config/mod.rs.
# ARGV: the URL, a path relative to it (inherit_from inside a remote file; empty for none), and
# the configured cache root (`--cache-root` or a CacheRootDirectory, as Store#early_cache_root
# picks it; empty for none).
# Prints the effective URL, a NUL, then the cache file path; on failure prints the exception's
# message to stderr and exits 1.
require 'net/http'
require 'time'
require 'digest/md5'
require 'fileutils'

class ConfigNotFoundError < StandardError; end

class RemoteConfig
  attr_reader :uri

  CACHE_LIFETIME = 24 * 60 * 60

  def initialize(url, cache_root)
    begin
      @uri = URI.parse(url)
    rescue URI::InvalidURIError
      raise ConfigNotFoundError, "Failed to resolve configuration: '#{url}' is not a valid URI"
    end
    @cache_root = cache_root
  end

  def file
    return cache_path unless cache_path_expired?

    request do |response|
      next if response.is_a?(Net::HTTPNotModified)
      next if response.is_a?(SocketError)

      FileUtils.mkdir_p(File.dirname(cache_path))
      File.write(cache_path, response.body)
    end

    cache_path
  end

  def inherit_from_remote(file)
    new_uri = @uri.dup
    new_uri.path.gsub!(%r{/[^/]*$}, "/#{file.delete_prefix('./')}")
    RemoteConfig.new(new_uri.to_s, @cache_root)
  end

  private

  def request(uri = @uri, limit = 10, &block)
    raise ArgumentError, 'HTTP redirect too deep' if limit.zero?

    http = Net::HTTP.new(uri.hostname, uri.port)
    http.use_ssl = uri.instance_of?(URI::HTTPS)

    generate_request(uri) do |request|
      handle_response(http.request(request), uri, limit, &block)
    rescue SocketError => e
      handle_response(e, uri, limit, &block)
    end
  end

  def generate_request(uri)
    request = Net::HTTP::Get.new(uri.request_uri)

    request.basic_auth(uri.user, uri.password) if uri.user
    request['If-Modified-Since'] = File.stat(cache_path).mtime.rfc2822 if cache_path_exists?

    yield request
  end

  def handle_response(response, uri, limit, &block)
    case response
    when Net::HTTPSuccess, Net::HTTPNotModified, SocketError
      yield response
    when Net::HTTPRedirection
      request(URI.join(uri, response['location']), limit - 1, &block)
    else
      begin
        response.error!
      rescue StandardError => e
        message = "#{e.message} while downloading remote config file #{cloned_url}"
        raise e, message
      end
    end
  end

  def cache_path
    @cache_path ||= File.expand_path(cache_name_from_uri, @cache_root)
  end

  def cache_path_exists?
    @cache_path_exists ||= File.exist?(cache_path)
  end

  def cache_path_expired?
    return true unless cache_path_exists?

    @cache_path_expired ||= begin
      file_age = (Time.now - File.stat(cache_path).mtime).to_f
      (file_age / CACHE_LIFETIME) > 1
    end
  end

  def cache_name_from_uri
    filename = File.basename(@uri.path).gsub(/\.ya?ml\z/i, '').byteslice(0, 217).scrub('')
    "#{filename}-#{Digest::MD5.hexdigest(@uri.to_s)}.yml"
  end

  def cloned_url
    uri = @uri.clone
    uri.user = nil if uri.user
    uri.password = nil if uri.password
    uri
  end
end

def cache_root(configured)
  root = ENV.fetch('RUBOCOP_CACHE_ROOT', nil)
  root ||= configured unless configured.empty?
  root ||= if ENV.key?('XDG_CACHE_HOME')
             File.join(ENV.fetch('XDG_CACHE_HOME'), Process.uid.to_s)
           else
             File.join(File.realpath(Dir.home), '.cache')
           end
  File.join(root, 'rubocop_cache')
end

url, relative, configured = ARGV
begin
  remote = RemoteConfig.new(url, cache_root(configured))
  remote = remote.inherit_from_remote(relative) unless relative.empty?
  path = remote.file
  $stdout.binmode
  print remote.uri.to_s, "\0", path
rescue StandardError => e
  $stderr.print e.message
  exit 1
end
