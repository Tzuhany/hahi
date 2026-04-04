// @generated
/// Generated client implementations.
pub mod agent_execution_service_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct AgentExecutionServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl AgentExecutionServiceClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> AgentExecutionServiceClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> AgentExecutionServiceClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            AgentExecutionServiceClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn send_message(
            &mut self,
            request: impl tonic::IntoRequest<super::SendMessageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SendMessageResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/hahi.chat.AgentExecutionService/SendMessage",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("hahi.chat.AgentExecutionService", "SendMessage"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn stream_events(
            &mut self,
            request: impl tonic::IntoRequest<super::StreamEventsRequest>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::super::events::EventFrame>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/hahi.chat.AgentExecutionService/StreamEvents",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("hahi.chat.AgentExecutionService", "StreamEvents"),
                );
            self.inner.server_streaming(req, path, codec).await
        }
        pub async fn resume_run(
            &mut self,
            request: impl tonic::IntoRequest<super::ResumeRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ResumeRunResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/hahi.chat.AgentExecutionService/ResumeRun",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("hahi.chat.AgentExecutionService", "ResumeRun"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn cancel_run(
            &mut self,
            request: impl tonic::IntoRequest<super::CancelRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CancelRunResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/hahi.chat.AgentExecutionService/CancelRun",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("hahi.chat.AgentExecutionService", "CancelRun"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_run_status(
            &mut self,
            request: impl tonic::IntoRequest<super::GetRunStatusRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetRunStatusResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/hahi.chat.AgentExecutionService/GetRunStatus",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("hahi.chat.AgentExecutionService", "GetRunStatus"),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod agent_execution_service_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with AgentExecutionServiceServer.
    #[async_trait]
    pub trait AgentExecutionService: std::marker::Send + std::marker::Sync + 'static {
        async fn send_message(
            &self,
            request: tonic::Request<super::SendMessageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SendMessageResponse>,
            tonic::Status,
        >;
        /// Server streaming response type for the StreamEvents method.
        type StreamEventsStream: tonic::codegen::tokio_stream::Stream<
                Item = std::result::Result<
                    super::super::events::EventFrame,
                    tonic::Status,
                >,
            >
            + std::marker::Send
            + 'static;
        async fn stream_events(
            &self,
            request: tonic::Request<super::StreamEventsRequest>,
        ) -> std::result::Result<
            tonic::Response<Self::StreamEventsStream>,
            tonic::Status,
        >;
        async fn resume_run(
            &self,
            request: tonic::Request<super::ResumeRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ResumeRunResponse>,
            tonic::Status,
        >;
        async fn cancel_run(
            &self,
            request: tonic::Request<super::CancelRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CancelRunResponse>,
            tonic::Status,
        >;
        async fn get_run_status(
            &self,
            request: tonic::Request<super::GetRunStatusRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetRunStatusResponse>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct AgentExecutionServiceServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> AgentExecutionServiceServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>>
    for AgentExecutionServiceServer<T>
    where
        T: AgentExecutionService,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/hahi.chat.AgentExecutionService/SendMessage" => {
                    #[allow(non_camel_case_types)]
                    struct SendMessageSvc<T: AgentExecutionService>(pub Arc<T>);
                    impl<
                        T: AgentExecutionService,
                    > tonic::server::UnaryService<super::SendMessageRequest>
                    for SendMessageSvc<T> {
                        type Response = super::SendMessageResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SendMessageRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as AgentExecutionService>::send_message(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SendMessageSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/hahi.chat.AgentExecutionService/StreamEvents" => {
                    #[allow(non_camel_case_types)]
                    struct StreamEventsSvc<T: AgentExecutionService>(pub Arc<T>);
                    impl<
                        T: AgentExecutionService,
                    > tonic::server::ServerStreamingService<super::StreamEventsRequest>
                    for StreamEventsSvc<T> {
                        type Response = super::super::events::EventFrame;
                        type ResponseStream = T::StreamEventsStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::StreamEventsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as AgentExecutionService>::stream_events(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = StreamEventsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.server_streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/hahi.chat.AgentExecutionService/ResumeRun" => {
                    #[allow(non_camel_case_types)]
                    struct ResumeRunSvc<T: AgentExecutionService>(pub Arc<T>);
                    impl<
                        T: AgentExecutionService,
                    > tonic::server::UnaryService<super::ResumeRunRequest>
                    for ResumeRunSvc<T> {
                        type Response = super::ResumeRunResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ResumeRunRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as AgentExecutionService>::resume_run(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ResumeRunSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/hahi.chat.AgentExecutionService/CancelRun" => {
                    #[allow(non_camel_case_types)]
                    struct CancelRunSvc<T: AgentExecutionService>(pub Arc<T>);
                    impl<
                        T: AgentExecutionService,
                    > tonic::server::UnaryService<super::CancelRunRequest>
                    for CancelRunSvc<T> {
                        type Response = super::CancelRunResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CancelRunRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as AgentExecutionService>::cancel_run(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = CancelRunSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/hahi.chat.AgentExecutionService/GetRunStatus" => {
                    #[allow(non_camel_case_types)]
                    struct GetRunStatusSvc<T: AgentExecutionService>(pub Arc<T>);
                    impl<
                        T: AgentExecutionService,
                    > tonic::server::UnaryService<super::GetRunStatusRequest>
                    for GetRunStatusSvc<T> {
                        type Response = super::GetRunStatusResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetRunStatusRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as AgentExecutionService>::get_run_status(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetRunStatusSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for AgentExecutionServiceServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "hahi.chat.AgentExecutionService";
    impl<T> tonic::server::NamedService for AgentExecutionServiceServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
/// Generated client implementations.
pub mod thread_service_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct ThreadServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl ThreadServiceClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> ThreadServiceClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> ThreadServiceClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            ThreadServiceClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn create_thread(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateThreadRequest>,
        ) -> std::result::Result<tonic::Response<super::ThreadProto>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/hahi.chat.ThreadService/CreateThread",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("hahi.chat.ThreadService", "CreateThread"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_thread(
            &mut self,
            request: impl tonic::IntoRequest<super::GetThreadRequest>,
        ) -> std::result::Result<tonic::Response<super::ThreadProto>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/hahi.chat.ThreadService/GetThread",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("hahi.chat.ThreadService", "GetThread"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_threads(
            &mut self,
            request: impl tonic::IntoRequest<super::ListThreadsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListThreadsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/hahi.chat.ThreadService/ListThreads",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("hahi.chat.ThreadService", "ListThreads"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn delete_thread(
            &mut self,
            request: impl tonic::IntoRequest<super::DeleteThreadRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DeleteThreadResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/hahi.chat.ThreadService/DeleteThread",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("hahi.chat.ThreadService", "DeleteThread"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_messages(
            &mut self,
            request: impl tonic::IntoRequest<super::ListMessagesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListMessagesResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/hahi.chat.ThreadService/ListMessages",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("hahi.chat.ThreadService", "ListMessages"));
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod thread_service_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with ThreadServiceServer.
    #[async_trait]
    pub trait ThreadService: std::marker::Send + std::marker::Sync + 'static {
        async fn create_thread(
            &self,
            request: tonic::Request<super::CreateThreadRequest>,
        ) -> std::result::Result<tonic::Response<super::ThreadProto>, tonic::Status>;
        async fn get_thread(
            &self,
            request: tonic::Request<super::GetThreadRequest>,
        ) -> std::result::Result<tonic::Response<super::ThreadProto>, tonic::Status>;
        async fn list_threads(
            &self,
            request: tonic::Request<super::ListThreadsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListThreadsResponse>,
            tonic::Status,
        >;
        async fn delete_thread(
            &self,
            request: tonic::Request<super::DeleteThreadRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DeleteThreadResponse>,
            tonic::Status,
        >;
        async fn list_messages(
            &self,
            request: tonic::Request<super::ListMessagesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListMessagesResponse>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct ThreadServiceServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> ThreadServiceServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for ThreadServiceServer<T>
    where
        T: ThreadService,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/hahi.chat.ThreadService/CreateThread" => {
                    #[allow(non_camel_case_types)]
                    struct CreateThreadSvc<T: ThreadService>(pub Arc<T>);
                    impl<
                        T: ThreadService,
                    > tonic::server::UnaryService<super::CreateThreadRequest>
                    for CreateThreadSvc<T> {
                        type Response = super::ThreadProto;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreateThreadRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ThreadService>::create_thread(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = CreateThreadSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/hahi.chat.ThreadService/GetThread" => {
                    #[allow(non_camel_case_types)]
                    struct GetThreadSvc<T: ThreadService>(pub Arc<T>);
                    impl<
                        T: ThreadService,
                    > tonic::server::UnaryService<super::GetThreadRequest>
                    for GetThreadSvc<T> {
                        type Response = super::ThreadProto;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetThreadRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ThreadService>::get_thread(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetThreadSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/hahi.chat.ThreadService/ListThreads" => {
                    #[allow(non_camel_case_types)]
                    struct ListThreadsSvc<T: ThreadService>(pub Arc<T>);
                    impl<
                        T: ThreadService,
                    > tonic::server::UnaryService<super::ListThreadsRequest>
                    for ListThreadsSvc<T> {
                        type Response = super::ListThreadsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListThreadsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ThreadService>::list_threads(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListThreadsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/hahi.chat.ThreadService/DeleteThread" => {
                    #[allow(non_camel_case_types)]
                    struct DeleteThreadSvc<T: ThreadService>(pub Arc<T>);
                    impl<
                        T: ThreadService,
                    > tonic::server::UnaryService<super::DeleteThreadRequest>
                    for DeleteThreadSvc<T> {
                        type Response = super::DeleteThreadResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DeleteThreadRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ThreadService>::delete_thread(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = DeleteThreadSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/hahi.chat.ThreadService/ListMessages" => {
                    #[allow(non_camel_case_types)]
                    struct ListMessagesSvc<T: ThreadService>(pub Arc<T>);
                    impl<
                        T: ThreadService,
                    > tonic::server::UnaryService<super::ListMessagesRequest>
                    for ListMessagesSvc<T> {
                        type Response = super::ListMessagesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListMessagesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ThreadService>::list_messages(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListMessagesSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for ThreadServiceServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "hahi.chat.ThreadService";
    impl<T> tonic::server::NamedService for ThreadServiceServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
